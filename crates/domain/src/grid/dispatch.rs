//! Merit-order dispatch and automatic generation control (AGC).
//!
//! The merit order ranks online dispatchable plants by marginal cost. AGC
//! spreads a system shortfall (positive: raise output) or surplus (negative:
//! lower output) over the auto-follow plants, cheapest first when raising and
//! most expensive first when lowering, each within its one-tick ramp.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::generation::Plant;
use crate::ids::PlantId;
use crate::units::Megawatts;

/// Below this a shortfall is treated as zero, and a leftover as fully allocated.
const EPSILON: f64 = 1e-9;

/// Cheapest first, ties broken by plant id so the order is total and stable.
fn by_cost_then_id(a: &Plant, b: &Plant) -> Ordering {
    a.spec()
        .marginal_cost
        .value()
        .total_cmp(&b.spec().marginal_cost.value())
        .then_with(|| a.id().cmp(&b.id()))
}

fn is_dispatchable_online(plant: &Plant) -> bool {
    plant.is_online() && plant.spec().kind.is_dispatchable()
}

/// Online dispatchable plants sorted by `(marginal_cost, id)`.
pub fn merit_order<'a>(plants: impl IntoIterator<Item = &'a Plant>) -> Vec<PlantId> {
    let mut online: Vec<&Plant> = plants
        .into_iter()
        .filter(|p| is_dispatchable_online(p))
        .collect();
    online.sort_by(|a, b| by_cost_then_id(a, b));
    online.into_iter().map(Plant::id).collect()
}

/// Spreads `shortfall` over the auto-follow, online, dispatchable plants.
///
/// A positive shortfall raises targets in ascending merit order; a negative
/// one lowers them in descending order. Each plant moves by at most its
/// one-tick headroom. `available` caps how high a plant may rise (a plant
/// missing from it is capped by its capacity). Participants that receive no
/// allocation are held at their current output. Plants that do not
/// participate are left untouched.
///
/// Returns the part of the shortfall that could not be allocated, with the
/// shortfall's sign (zero when it was fully allocated).
pub fn agc_retarget(
    plants: &mut [&mut Plant],
    available: &BTreeMap<PlantId, Megawatts>,
    shortfall: Megawatts,
) -> Megawatts {
    let raising = shortfall.value() > 0.0;
    let mut remaining = if shortfall.value().abs() < EPSILON {
        0.0
    } else {
        shortfall.value().abs()
    };

    let mut order: Vec<usize> = (0..plants.len())
        .filter(|&i| plants[i].auto_follow() && is_dispatchable_online(plants[i]))
        .collect();
    order.sort_by(|&a, &b| by_cost_then_id(plants[a], plants[b]));
    if !raising {
        order.reverse();
    }

    for i in order {
        let plant = &mut *plants[i];
        let output = plant.output();
        let alloc = if remaining > 0.0 {
            let headroom = if raising {
                let cap = available
                    .get(&plant.id())
                    .copied()
                    .unwrap_or(plant.available_capacity());
                plant.headroom_up(cap)
            } else {
                plant.headroom_down()
            };
            headroom.value().min(remaining)
        } else {
            0.0
        };
        remaining -= alloc;
        if remaining < EPSILON {
            remaining = 0.0;
        }
        let signed = if raising { alloc } else { -alloc };
        // A plant pulled below minimum stable output by its resource (dry
        // hydro) cannot hold that output as a target, so clamp into bounds.
        let target =
            (output + Megawatts::new(signed)).clamp(plant.min_output(), plant.available_capacity());
        let result = plant.set_target(target);
        debug_assert!(result.is_ok(), "AGC target {target:?} rejected: {result:?}");
    }

    let leftover = Megawatts::new(remaining);
    if raising { leftover } else { -leftover }
}

/// Sets the balanced state at instant 0 of a representative day: a pure
/// merit-order fill of `load` over the auto-follow, online, dispatchable
/// plants in `plants` (the caller passes only connected ones).
///
/// Each participant starts at minimum stable output; output is then added in
/// ascending `(marginal_cost, id)` order, each plant up to
/// `min(available, capacity)` (a plant missing from `available` is capped by
/// capacity), until `load` is met. Output and target are set together,
/// directly: this is a synthetic boundary state, so there is no ramp limit and
/// no events. Minimum outputs that already exceed `load` are left as they are;
/// AGC lowers them after the first tick within the ramp limits. Plants that
/// do not participate are untouched.
///
/// Returns the load the participants could not meet (zero when it was met).
pub fn boundary_dispatch(
    plants: &mut [&mut Plant],
    available: &BTreeMap<PlantId, Megawatts>,
    load: Megawatts,
) -> Megawatts {
    let mut order: Vec<usize> = (0..plants.len())
        .filter(|&i| plants[i].auto_follow() && is_dispatchable_online(plants[i]))
        .collect();
    order.sort_by(|&a, &b| by_cost_then_id(plants[a], plants[b]));

    let minimum: f64 = order.iter().map(|&i| plants[i].min_output().value()).sum();
    let mut remaining = load.value() - minimum;
    for i in order {
        let plant = &mut *plants[i];
        let min = plant.min_output();
        let cap = available
            .get(&plant.id())
            .copied()
            .unwrap_or(plant.available_capacity())
            .min(plant.available_capacity());
        let room = (cap - min).value().max(0.0);
        let add = room.min(remaining.max(0.0));
        remaining -= add;
        plant.set_boundary_output(min + Megawatts::new(add));
    }
    Megawatts::new(if remaining < EPSILON { 0.0 } else { remaining })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation::PlantStatus;
    use crate::ids::{CellCoord, NodeId};
    use crate::region::fixtures::tiny_region;
    use crate::region::{PlantKind, PlantSpec};
    use crate::time::SimMonth;
    use crate::units::Price;
    use proptest::prelude::*;

    /// A commissioned plant built from `spec`, brought online at minimum output.
    fn online_from_spec(id: u32, spec: PlantSpec) -> Plant {
        let mut p = Plant::new_commissioned(
            PlantId(id),
            NodeId(id),
            CellCoord { x: 7, y: 3 },
            spec,
            SimMonth::new(2027, 4).unwrap(),
        );
        if p.is_online() {
            return p;
        }
        p.start().unwrap();
        while !p.is_online() {
            p.step(p.spec().capacity);
        }
        p
    }

    fn online_plant(id: u32, kind: PlantKind) -> Plant {
        online_from_spec(id, tiny_region().spec(kind).clone())
    }

    /// Ramps an online dispatchable plant to `min + fraction * (cap - min)`.
    fn ramp_to_fraction(p: &mut Plant, fraction: f64) {
        let min = p.spec().min_output();
        let goal = min + (p.spec().capacity - min) * fraction;
        p.set_target(goal).unwrap();
        for _ in 0..10_000 {
            if p.output() == goal {
                return;
            }
            p.step(p.spec().capacity);
        }
        panic!("plant never reached {goal:?}");
    }

    fn follower(id: u32, kind: PlantKind) -> Plant {
        let mut p = online_plant(id, kind);
        p.set_auto_follow(true).unwrap();
        p
    }

    fn target_of(p: &Plant) -> Megawatts {
        let PlantStatus::Online { target, .. } = p.status() else {
            panic!("plant {:?} not online", p.id());
        };
        target
    }

    fn none_available() -> BTreeMap<PlantId, Megawatts> {
        BTreeMap::new()
    }

    #[test]
    fn merit_order_sorts_by_cost_then_id_and_skips_solar_and_offline() {
        let nuclear = online_plant(4, PlantKind::Nuclear);
        let coal = online_plant(3, PlantKind::Coal);
        let ccgt = online_plant(2, PlantKind::Ccgt);
        let peaker = online_plant(1, PlantKind::Peaker);
        let solar = online_plant(5, PlantKind::Solar);
        let offline_ccgt = Plant::new_commissioned(
            PlantId(6),
            NodeId(6),
            CellCoord { x: 7, y: 3 },
            tiny_region().spec(PlantKind::Ccgt).clone(),
            SimMonth::new(2027, 4).unwrap(),
        );
        let plants = [peaker, ccgt, solar, offline_ccgt, coal, nuclear];
        assert_eq!(
            merit_order(&plants),
            vec![PlantId(4), PlantId(3), PlantId(2), PlantId(1)]
        );
    }

    #[test]
    fn merit_order_breaks_cost_ties_by_id() {
        let tied = |id: u32, kind: PlantKind| {
            let mut spec = tiny_region().spec(kind).clone();
            spec.marginal_cost = Price::per_mwh(50.0);
            online_from_spec(id, spec)
        };
        let (a, b) = (tied(9, PlantKind::Ccgt), tied(2, PlantKind::Coal));
        assert_eq!(merit_order([&a, &b]), vec![PlantId(2), PlantId(9)]);
    }

    #[test]
    fn raises_cheapest_first_within_ramp() {
        let mut ccgt = follower(1, PlantKind::Ccgt);
        let mut peaker = follower(2, PlantKind::Peaker);
        let (c0, p0) = (ccgt.output(), peaker.output());
        let left = agc_retarget(
            &mut [&mut peaker, &mut ccgt],
            &none_available(),
            Megawatts::new(50.0),
        );
        assert_eq!(target_of(&ccgt), c0 + Megawatts::new(25.0));
        assert_eq!(target_of(&peaker), p0 + Megawatts::new(25.0));
        assert_eq!(left, Megawatts::ZERO);
    }

    #[test]
    fn returns_unallocated_when_headroom_exhausted() {
        let mut ccgt = follower(1, PlantKind::Ccgt);
        let mut peaker = follower(2, PlantKind::Peaker);
        let left = agc_retarget(
            &mut [&mut ccgt, &mut peaker],
            &none_available(),
            Megawatts::new(1000.0),
        );
        assert_eq!(left, Megawatts::new(935.0));
    }

    #[test]
    fn lowers_most_expensive_first_and_returns_unallocated_surplus() {
        let mut ccgt = follower(1, PlantKind::Ccgt);
        let mut peaker = follower(2, PlantKind::Peaker);
        ramp_to_fraction(&mut ccgt, 1.0);
        ramp_to_fraction(&mut peaker, 1.0);
        let (c0, p0) = (ccgt.output(), peaker.output());
        let left = agc_retarget(
            &mut [&mut ccgt, &mut peaker],
            &none_available(),
            Megawatts::new(-50.0),
        );
        assert_eq!(target_of(&peaker), p0 - Megawatts::new(40.0));
        assert_eq!(target_of(&ccgt), c0 - Megawatts::new(10.0));
        assert_eq!(left, Megawatts::ZERO);

        let left = agc_retarget(
            &mut [&mut ccgt, &mut peaker],
            &none_available(),
            Megawatts::new(-500.0),
        );
        assert_eq!(left, Megawatts::new(-435.0));
    }

    #[test]
    fn unallocated_plants_are_held_at_output() {
        let mut ccgt = follower(1, PlantKind::Ccgt);
        let mut peaker = follower(2, PlantKind::Peaker);
        ramp_to_fraction(&mut peaker, 0.5);
        peaker.set_target(peaker.spec().capacity).unwrap();
        let left = agc_retarget(
            &mut [&mut ccgt, &mut peaker],
            &none_available(),
            Megawatts::new(10.0),
        );
        assert_eq!(left, Megawatts::ZERO);
        assert_eq!(target_of(&peaker), peaker.output());
    }

    #[test]
    fn non_participants_are_untouched() {
        let mut manual = online_plant(1, PlantKind::Ccgt);
        manual.set_target(Megawatts::new(300.0)).unwrap();
        let mut solar = online_plant(2, PlantKind::Solar);
        let mut offline = Plant::new_commissioned(
            PlantId(3),
            NodeId(3),
            CellCoord { x: 0, y: 0 },
            tiny_region().spec(PlantKind::Ccgt).clone(),
            SimMonth::new(2027, 4).unwrap(),
        );
        offline.set_auto_follow(true).unwrap();
        let before = (manual.clone(), solar.clone(), offline.clone());
        let left = agc_retarget(
            &mut [&mut manual, &mut solar, &mut offline],
            &none_available(),
            Megawatts::new(100.0),
        );
        assert_eq!(left, Megawatts::new(100.0));
        assert_eq!((manual, solar, offline), before);
    }

    #[test]
    fn availability_caps_the_raise() {
        let mut ccgt = follower(1, PlantKind::Ccgt);
        let out = ccgt.output();
        let avail = BTreeMap::from([(PlantId(1), out + Megawatts::new(10.0))]);
        let left = agc_retarget(&mut [&mut ccgt], &avail, Megawatts::new(20.0));
        assert_eq!(target_of(&ccgt), out + Megawatts::new(10.0));
        assert_eq!(left, Megawatts::new(10.0));
    }

    #[test]
    fn negligible_shortfall_holds_everyone() {
        let mut ccgt = follower(1, PlantKind::Ccgt);
        ramp_to_fraction(&mut ccgt, 0.5);
        ccgt.set_target(ccgt.spec().capacity).unwrap();
        let left = agc_retarget(&mut [&mut ccgt], &none_available(), Megawatts::new(1e-12));
        assert_eq!(left, Megawatts::ZERO);
        assert_eq!(target_of(&ccgt), ccgt.output());
    }

    fn output_of(p: &Plant) -> Megawatts {
        p.output()
    }

    /// Nuclear (min 770, cap 1100, 10 $/MWh), CCGT (min 200, cap 500, 45) and
    /// peaker (min 40, cap 200, 150), all online and auto-follow.
    fn boundary_fleet() -> [Plant; 3] {
        [
            follower(3, PlantKind::Peaker),
            follower(1, PlantKind::Nuclear),
            follower(2, PlantKind::Ccgt),
        ]
    }

    #[test]
    fn boundary_dispatch_fills_in_merit_order() {
        let mut plants = boundary_fleet();
        let [peaker, nuclear, ccgt] = &mut plants;
        let unmet = boundary_dispatch(
            &mut [peaker, nuclear, ccgt],
            &none_available(),
            Megawatts::new(1400.0),
        );
        let [peaker, nuclear, ccgt] = &plants;
        assert_eq!(unmet, Megawatts::ZERO);
        // Cheapest first: nuclear to capacity, ccgt takes the rest, peaker stays at minimum.
        assert_eq!(output_of(nuclear), Megawatts::new(1100.0));
        assert_eq!(output_of(ccgt), Megawatts::new(260.0));
        assert_eq!(output_of(peaker), Megawatts::new(40.0));
        let total: Megawatts = plants.iter().map(output_of).sum();
        assert!((total - Megawatts::new(1400.0)).abs().value() < 1e-9);
        // The target moves with the output, so AGC starts from a settled plant.
        for p in &plants {
            assert_eq!(target_of(p), p.output());
        }
    }

    #[test]
    fn boundary_dispatch_leaves_manual_plants_at_initial_target() {
        let mut manual = online_plant(1, PlantKind::Nuclear);
        manual.set_target(Megawatts::new(900.0)).unwrap();
        let mut follower_plant = follower(2, PlantKind::Ccgt);
        let mut solar = online_plant(3, PlantKind::Solar);
        let before = (manual.clone(), solar.clone());
        let unmet = boundary_dispatch(
            &mut [&mut manual, &mut follower_plant, &mut solar],
            &none_available(),
            Megawatts::new(450.0),
        );
        assert_eq!(unmet, Megawatts::ZERO);
        assert_eq!((manual, solar), before);
        assert_eq!(follower_plant.output(), Megawatts::new(450.0));
    }

    #[test]
    fn boundary_dispatch_ignores_plants_that_are_not_online() {
        let mut offline = Plant::new_commissioned(
            PlantId(1),
            NodeId(1),
            CellCoord { x: 0, y: 0 },
            tiny_region().spec(PlantKind::Ccgt).clone(),
            SimMonth::new(2027, 4).unwrap(),
        );
        offline.set_auto_follow(true).unwrap();
        let before = offline.clone();
        let unmet = boundary_dispatch(
            &mut [&mut offline],
            &none_available(),
            Megawatts::new(100.0),
        );
        assert_eq!(unmet, Megawatts::new(100.0));
        assert_eq!(offline, before);
    }

    #[test]
    fn boundary_dispatch_reports_unmet_load() {
        let mut plants = boundary_fleet();
        let [peaker, nuclear, ccgt] = &mut plants;
        let unmet = boundary_dispatch(
            &mut [peaker, nuclear, ccgt],
            &none_available(),
            Megawatts::new(2000.0),
        );
        assert!((unmet - Megawatts::new(200.0)).abs().value() < 1e-9);
        for p in &plants {
            assert_eq!(p.output(), p.spec().capacity);
        }
    }

    #[test]
    fn boundary_dispatch_surplus_minimum_output_is_left_as_is() {
        let mut plants = boundary_fleet();
        let [peaker, nuclear, ccgt] = &mut plants;
        let unmet = boundary_dispatch(
            &mut [peaker, nuclear, ccgt],
            &none_available(),
            Megawatts::new(100.0),
        );
        assert_eq!(unmet, Megawatts::ZERO);
        for p in &plants {
            assert_eq!(p.output(), p.spec().min_output());
        }
    }

    #[test]
    fn boundary_dispatch_respects_availability() {
        let mut nuclear = follower(1, PlantKind::Nuclear);
        let mut ccgt = follower(2, PlantKind::Ccgt);
        let avail = BTreeMap::from([(PlantId(1), Megawatts::new(900.0))]);
        let unmet = boundary_dispatch(
            &mut [&mut nuclear, &mut ccgt],
            &avail,
            Megawatts::new(1200.0),
        );
        assert_eq!(nuclear.output(), Megawatts::new(900.0));
        assert_eq!(ccgt.output(), Megawatts::new(300.0));
        assert_eq!(unmet, Megawatts::ZERO);
    }

    /// A follower whose minimum stable output is zero, online at 0 MW.
    fn zero_minimum_follower(id: u32) -> Plant {
        let mut spec = tiny_region().spec(PlantKind::Ccgt).clone();
        spec.min_stable = crate::units::Ratio::ZERO;
        let mut p = online_from_spec(id, spec);
        p.set_auto_follow(true).unwrap();
        p
    }

    #[test]
    fn shortfall_of_exactly_epsilon_is_still_allocated() {
        let mut ccgt = follower(1, PlantKind::Ccgt);
        let out = ccgt.output();
        let left = agc_retarget(&mut [&mut ccgt], &none_available(), Megawatts::new(EPSILON));
        assert_eq!(left, Megawatts::ZERO);
        assert!(target_of(&ccgt) > out);
    }

    #[test]
    fn leftover_of_exactly_epsilon_is_reported_not_snapped() {
        // Online at zero minimum with no headroom: nothing can absorb the shortfall.
        let mut ccgt = zero_minimum_follower(1);
        let avail = BTreeMap::from([(PlantId(1), Megawatts::ZERO)]);
        let left = agc_retarget(&mut [&mut ccgt], &avail, Megawatts::new(EPSILON));
        assert_eq!(left, Megawatts::new(EPSILON));
        let left = agc_retarget(&mut [&mut ccgt], &avail, Megawatts::new(-EPSILON));
        assert_eq!(left, Megawatts::new(-EPSILON));
    }

    #[test]
    fn sub_epsilon_residue_after_allocation_is_snapped_to_zero() {
        let mut ccgt = follower(1, PlantKind::Ccgt);
        // One tick of CCGT ramp is 25 MW; 5e-10 MW more cannot be allocated but is noise.
        let left = agc_retarget(
            &mut [&mut ccgt],
            &none_available(),
            Megawatts::new(25.0 + 5e-10),
        );
        assert_eq!(left, Megawatts::ZERO);
    }

    #[test]
    fn boundary_unmet_of_exactly_epsilon_is_reported_not_snapped() {
        let mut ccgt = zero_minimum_follower(1);
        let avail = BTreeMap::from([(PlantId(1), Megawatts::ZERO)]);
        let unmet = boundary_dispatch(&mut [&mut ccgt], &avail, Megawatts::new(EPSILON));
        assert_eq!(unmet, Megawatts::new(EPSILON));
        let unmet = boundary_dispatch(&mut [&mut ccgt], &avail, Megawatts::new(EPSILON / 2.0));
        assert_eq!(unmet, Megawatts::ZERO);
    }

    const KINDS: [PlantKind; 4] = [
        PlantKind::Nuclear,
        PlantKind::Coal,
        PlantKind::Ccgt,
        PlantKind::Peaker,
    ];

    fn spec_plant(index: usize, cost: f64, fraction: f64) -> Plant {
        let mut spec = tiny_region().spec(KINDS[index % KINDS.len()]).clone();
        spec.marginal_cost = Price::per_mwh(cost);
        let mut p = online_from_spec(index as u32 + 1, spec);
        p.set_auto_follow(true).unwrap();
        ramp_to_fraction(&mut p, fraction);
        p
    }

    proptest! {
        #[test]
        fn never_raises_expensive_while_cheaper_has_headroom(
            specs in prop::collection::vec((1.0f64..300.0, 0.0f64..1.0), 1..8),
            shortfall in -500.0f64..500.0,
        ) {
            let mut plants: Vec<Plant> = specs
                .iter()
                .enumerate()
                .map(|(i, &(cost, fraction))| spec_plant(i, cost, fraction))
                .collect();
            let raising = shortfall > 0.0;
            let headroom: Vec<f64> = plants
                .iter()
                .map(|p| {
                    if raising {
                        p.headroom_up(p.spec().capacity).value()
                    } else {
                        p.headroom_down().value()
                    }
                })
                .collect();
            let outputs: Vec<f64> = plants.iter().map(|p| p.output().value()).collect();

            let mut refs: Vec<&mut Plant> = plants.iter_mut().collect();
            let left = agc_retarget(&mut refs, &none_available(), Megawatts::new(shortfall));

            let allocs: Vec<f64> = plants
                .iter()
                .zip(&outputs)
                .map(|(p, out)| (target_of(p).value() - out).abs())
                .collect();
            for (i, p) in plants.iter().enumerate() {
                let moved = target_of(p).value() - outputs[i];
                let right_way = if raising { moved >= 0.0 } else { moved <= 0.0 };
                prop_assert!(right_way);
                for (j, q) in plants.iter().enumerate() {
                    let (ci, cj) = (p.spec().marginal_cost.value(), q.spec().marginal_cost.value());
                    // Raising favours the cheaper plant; lowering favours the dearer one.
                    let (favoured, other) = if raising { (ci < cj, j) } else { (ci > cj, j) };
                    if favoured && allocs[other] > 1e-9 {
                        prop_assert!(headroom[i] - allocs[i] <= 1e-9);
                    }
                }
            }
            let total_alloc: f64 = allocs.iter().sum();
            let total_headroom: f64 = headroom.iter().sum();
            prop_assert!((total_alloc - shortfall.abs().min(total_headroom)).abs() <= 1e-6);
            prop_assert!((left.value().abs() - (shortfall.abs() - total_alloc)).abs() <= 1e-6);
        }
    }
}

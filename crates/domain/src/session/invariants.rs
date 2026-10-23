//! Whole-session invariants, checked by tests and debug tooling. A violation
//! is a bug in the simulation, never a consequence of player input.

use thiserror::Error;

use super::GameSession;
use crate::generation::{Plant, PlantStatus};
use crate::grid::{GridMode, TickSummary};
use crate::ids::PlantId;
use crate::region::PlantKind;
use crate::units::{Megawatts, Money, Ratio};

/// Tolerance for float comparisons, in MW. Tick quantities are sums of a few
/// dozen terms of order 1e3 MW, so honest rounding stays far below this.
const TOLERANCE: f64 = 1e-6;

/// The first invariant found broken.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum InvariantViolation {
    #[error("cash moved by {cash_delta:?} but the ledger sums to {ledger_sum:?}")]
    CashLedgerMismatch {
        cash_delta: Money,
        ledger_sum: Money,
    },
    #[error("plant {plant:?} output is outside its bounds")]
    PlantOutOfBounds { plant: PlantId },
    #[error("tick {tick} does not conserve energy")]
    ConservationBroken { tick: u32 },
    #[error("tick {tick} does not match the live plant outputs")]
    GenerationMismatch { tick: u32 },
    #[error("a rotating outage of {outage:?} is in effect during a blackout")]
    OutageDuringBlackout { outage: Ratio },
}

impl GameSession {
    /// Checks, in order: that cash less starting cash equals the ledger sum;
    /// that every online dispatchable plant's output lies within
    /// `[min_output, capacity]` (hydro may fall to zero, since its reservoir
    /// caps output below minimum); that the last tick summary satisfies
    /// `generation = Σ by_plant = Σ by_kind = served + battery_charge + imbalance`,
    /// `served + unserved = demand`, `0 ≤ served ≤ demand`,
    /// `demand × outage ≤ unserved` (within `1e-6 × demand`) and
    /// `curtailed ≥ 0`; and that its `by_plant` is exactly the connected,
    /// commissioned plants at their live outputs, so `generation` is what the
    /// plants actually produce. Also that no rotating outage is in effect
    /// during a blackout, where all dark load is protection's (spec §15.3).
    pub fn check_invariants(&self) -> Result<(), InvariantViolation> {
        let cash_delta = self.company.cash() - self.company.starting_cash();
        let ledger_sum = self.company.ledger_sum();
        if cash_delta != ledger_sum {
            return Err(InvariantViolation::CashLedgerMismatch {
                cash_delta,
                ledger_sum,
            });
        }
        for plant in self.power.plants().values() {
            if let (Some(spec), Some(b)) = (plant.spec().battery, plant.battery()) {
                let f = b.flow();
                if b.stored().value() < -TOLERANCE
                    || b.stored().value() > spec.energy_capacity.value() + TOLERANCE
                    || f.charge.value() < 0.0
                    || f.charge > spec.charge_power
                    || f.discharge.value() < 0.0
                    || f.discharge > plant.spec().capacity
                    || (f.charge > Megawatts::ZERO && f.discharge > Megawatts::ZERO)
                {
                    return Err(InvariantViolation::PlantOutOfBounds { plant: plant.id() });
                }
            }
            let PlantStatus::Online { output, .. } = plant.status() else {
                continue;
            };
            let spec = plant.spec();
            if !spec.kind.is_dispatchable() {
                continue;
            }
            let min = if spec.kind == PlantKind::Hydro {
                Megawatts::ZERO
            } else {
                spec.min_output()
            };
            let below = min.value() - output.value() > TOLERANCE;
            let above = output.value() - spec.capacity.value() > TOLERANCE;
            if below || above {
                return Err(InvariantViolation::PlantOutOfBounds { plant: plant.id() });
            }
        }
        let outage = self.power.outage();
        if self.power.protection().mode == GridMode::Blackout && outage != Ratio::ZERO {
            return Err(InvariantViolation::OutageDuringBlackout { outage });
        }
        match self.day_ticks.last() {
            Some(summary) if !conserves(summary) => {
                Err(InvariantViolation::ConservationBroken { tick: summary.tick })
            }
            Some(summary) if !self.reports_live_outputs(summary) => {
                Err(InvariantViolation::GenerationMismatch { tick: summary.tick })
            }
            _ => Ok(()),
        }
    }

    /// Whether `s.by_plant` holds exactly the connected, commissioned plants,
    /// each at its live output. The summary copies those outputs, so they
    /// compare exactly. The one command that can move an output after a tick
    /// is a zero-length manual start, straight from 0 MW to online.
    fn reports_live_outputs(&self, s: &TickSummary) -> bool {
        let network = self.power.network();
        let live: Vec<&Plant> = self
            .power
            .plants()
            .values()
            .filter(|p| {
                !matches!(p.status(), PlantStatus::UnderConstruction { .. })
                    && network.is_connected(p.node())
            })
            .collect();
        live.len() == s.by_plant.len()
            && live.iter().all(|p| {
                s.by_plant.get(&p.id()).is_some_and(|&reported| {
                    reported == p.output()
                        || (p.battery().is_some_and(|b| b.flow_overridden())
                            && p.output() == Megawatts::ZERO)
                        || (reported == Megawatts::ZERO && p.spec().startup.ticks() == 0)
                })
            })
    }
}

/// The planned outage, `demand × outage`, is part of unserved energy, never
/// added to it: it may exceed `unserved` only by float noise, `1e-6 × demand`.
/// (A slightly negative `unserved` with no outage is `served ≤ demand`'s
/// tolerance, not this check's.)
fn planned_is_within_unserved(s: &TickSummary) -> bool {
    let planned = s.demand.value() * s.outage.value();
    planned - s.unserved.value().max(0.0) <= TOLERANCE * s.demand.value()
}

fn conserves(s: &TickSummary) -> bool {
    let near = |a: Megawatts, b: Megawatts| (a - b).value().abs() <= TOLERANCE;
    // `a ≥ b`, within the tolerance.
    let at_least = |a: Megawatts, b: Megawatts| b.value() - a.value() <= TOLERANCE;
    let by_plant: Megawatts = s.by_plant.values().copied().sum();
    let by_kind: Megawatts = s.by_kind.values().copied().sum();
    near(s.generation, by_plant)
        && near(s.generation, by_kind)
        && near(s.generation, s.served + s.battery_charge + s.imbalance)
        && near(s.served + s.unserved, s.demand)
        && planned_is_within_unserved(s)
        && at_least(s.served, Megawatts::ZERO)
        && at_least(s.demand, s.served)
        && at_least(s.curtailed, Megawatts::ZERO)
        && at_least(s.battery_charge, Megawatts::ZERO)
        && near(
            s.battery_charge,
            s.batteries.values().map(|f| f.charge).sum(),
        )
        && near(
            s.battery_discharge,
            s.batteries.values().map(|f| f.discharge).sum(),
        )
        && s.batteries.iter().all(|(id, f)| {
            let expected = f.opening.value() + f.charge.value() / 60.0
                - f.charge_loss.value()
                - f.discharge.value() / 60.0
                - f.discharge_loss.value();
            (expected - f.stored.value()).abs() <= TOLERANCE
                && f.stored.value() >= 0.0
                && (f.charge == Megawatts::ZERO || f.discharge == Megawatts::ZERO)
                && near(
                    s.by_plant.get(id).copied().unwrap_or(Megawatts::ZERO),
                    f.discharge,
                )
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::*;
    use crate::grid::LineStatus;
    use crate::ids::{CellCoord, LineId};
    use crate::region::RegionDefinition;
    use crate::region::fixtures::tiny_region_parts;
    use crate::session::GameSettings;
    use crate::time::SimDuration;
    use crate::units::Hertz;

    fn session_with(edit: impl FnOnce(&mut crate::region::RegionDefinitionParts)) -> GameSession {
        let mut parts = tiny_region_parts();
        edit(&mut parts);
        let region = RegionDefinition::try_from(parts).unwrap();
        GameSession::new(Arc::new(region), 1, GameSettings::default())
    }

    fn plant_id(s: &GameSession, kind: PlantKind) -> PlantId {
        *s.power
            .plants()
            .iter()
            .find(|(_, p)| p.kind() == kind)
            .map(|(id, _)| id)
            .expect("plant of kind")
    }

    fn set_output(s: &mut GameSession, id: PlantId, mw: f64) {
        s.power
            .plant_mut(id)
            .unwrap()
            .set_output_unchecked(Megawatts::new(mw));
    }

    #[test]
    fn a_fresh_session_satisfies_every_invariant() {
        assert_eq!(session_with(|_| {}).check_invariants(), Ok(()));
    }

    #[test]
    fn no_rotating_outage_is_in_effect_during_a_blackout() {
        let mut s = session_with(|_| {});
        let share = Ratio::new(0.3).unwrap();
        // An outage in Normal mode, and a blackout with none, are both fine.
        s.power.set_outage_unchecked(GridMode::Normal, share);
        assert_eq!(s.check_invariants(), Ok(()));
        s.power
            .set_outage_unchecked(GridMode::Blackout, Ratio::ZERO);
        assert_eq!(s.check_invariants(), Ok(()));
        s.power.set_outage_unchecked(GridMode::Blackout, share);
        assert_eq!(
            s.check_invariants(),
            Err(InvariantViolation::OutageDuringBlackout { outage: share })
        );
    }

    #[test]
    fn online_dispatchable_output_must_lie_within_bounds() {
        let mut s = session_with(|_| {});
        let coal = plant_id(&s, PlantKind::Coal);
        let violation = Err(InvariantViolation::PlantOutOfBounds { plant: coal });
        // Coal: minimum 240 MW, capacity 600 MW.
        for (mw, ok) in [
            (240.0, true),
            (600.0, true),
            (239.9, false),
            (0.0, false),
            (600.1, false),
        ] {
            set_output(&mut s, coal, mw);
            let expected = if ok { Ok(()) } else { violation.clone() };
            assert_eq!(s.check_invariants(), expected, "{mw} MW");
        }
        // Half a micro-megawatt of float noise is tolerated either side.
        set_output(&mut s, coal, 240.0 - 5e-7);
        assert_eq!(s.check_invariants(), Ok(()));
        set_output(&mut s, coal, 600.0 + 5e-7);
        assert_eq!(s.check_invariants(), Ok(()));
    }

    #[test]
    fn the_tolerance_is_inclusive_at_exactly_one_micro_megawatt() {
        // A minimum of exactly TOLERANCE with zero output sits exactly TOLERANCE below it;
        // a capacity of TOLERANCE with 2 * TOLERANCE output sits exactly TOLERANCE above
        // it (doubling and the subtraction are both exact). Both pass.
        let mut s = session_with(|parts| {
            for spec in &mut parts.catalog {
                if spec.kind == PlantKind::Coal {
                    spec.capacity = Megawatts::new(1.0);
                    spec.min_stable = Ratio::new(TOLERANCE).unwrap();
                }
            }
        });
        let coal = plant_id(&s, PlantKind::Coal);
        set_output(&mut s, coal, 0.0);
        assert_eq!(s.check_invariants(), Ok(()));

        let mut s = session_with(|parts| {
            for spec in &mut parts.catalog {
                if spec.kind == PlantKind::Coal {
                    spec.capacity = Megawatts::new(TOLERANCE);
                    spec.min_stable = Ratio::ZERO;
                }
            }
        });
        let coal = plant_id(&s, PlantKind::Coal);
        set_output(&mut s, coal, 2.0 * TOLERANCE);
        assert_eq!(s.check_invariants(), Ok(()));
    }

    #[test]
    fn a_dry_hydro_plant_may_sit_below_its_minimum_but_not_other_kinds() {
        let mut s = session_with(|parts| {
            for spec in &mut parts.catalog {
                if spec.kind == PlantKind::Hydro {
                    spec.min_stable = Ratio::new(0.3).unwrap();
                }
            }
        });
        let hydro = s.insert_commissioned_plant(PlantKind::Hydro, CellCoord { x: 10, y: 1 });
        set_output(&mut s, hydro, 0.0);
        assert_eq!(s.check_invariants(), Ok(()));
        // Above capacity is wrong for hydro too.
        set_output(&mut s, hydro, 800.1);
        assert_eq!(
            s.check_invariants(),
            Err(InvariantViolation::PlantOutOfBounds { plant: hydro })
        );
    }

    #[test]
    fn solar_output_is_not_bounded_below_by_a_minimum() {
        let mut s = session_with(|_| {});
        let solar = s.insert_commissioned_plant(PlantKind::Solar, CellCoord { x: 4, y: 4 });
        set_output(&mut s, solar, 9_999.0);
        assert_eq!(s.check_invariants(), Ok(()));
    }

    fn summary(
        demand: f64,
        served: f64,
        generation: f64,
        imbalance: f64,
        by_plant: &[f64],
    ) -> TickSummary {
        TickSummary {
            tick: 7,
            demand: Megawatts::new(demand),
            served: Megawatts::new(served),
            unserved: Megawatts::new(demand - served),
            generation: Megawatts::new(generation),
            battery_charge: Megawatts::ZERO,
            battery_discharge: Megawatts::ZERO,
            batteries: Default::default(),
            curtailed: Megawatts::ZERO,
            imbalance: Megawatts::new(imbalance),
            by_kind: BTreeMap::from([(PlantKind::Coal, Megawatts::new(by_plant.iter().sum()))]),
            by_plant: by_plant
                .iter()
                .enumerate()
                .map(|(i, mw)| (PlantId(i as u32), Megawatts::new(*mw)))
                .collect(),
            frequency: Hertz::new(60.0),
            mode: GridMode::Normal,
            shed: Ratio::ZERO,
            outage: Ratio::ZERO,
        }
    }

    #[test]
    fn conservation_needs_all_three_identities() {
        // demand 100, served 90, 95 generated by two plants: imbalance +5, unserved 10.
        assert!(conserves(&summary(100.0, 90.0, 95.0, 5.0, &[60.0, 35.0])));
        // generation differs from the sum over plants
        assert!(!conserves(&summary(100.0, 90.0, 95.0, 5.0, &[60.0, 30.0])));
        // generation differs from served + imbalance
        assert!(!conserves(&summary(100.0, 90.0, 95.0, 4.0, &[60.0, 35.0])));
        assert!(!conserves(&summary(100.0, 90.0, 95.0, -5.0, &[60.0, 35.0])));
        // served + unserved differs from demand
        let mut broken = summary(100.0, 90.0, 95.0, 5.0, &[60.0, 35.0]);
        broken.unserved = Megawatts::new(11.0);
        assert!(!conserves(&broken));
        // Differences inside the tolerance are float noise.
        let mut noisy = summary(100.0, 90.0, 95.0, 5.0, &[60.0, 35.0]);
        noisy.generation = Megawatts::new(95.0 + 5e-7);
        assert!(conserves(&noisy));
    }

    #[test]
    fn conservation_also_needs_by_kind_served_bounds_and_curtailment() {
        let t = TOLERANCE;
        let ok = summary(100.0, 90.0, 95.0, 5.0, &[60.0, 35.0]);
        assert!(conserves(&ok));

        // Σ by_kind must equal generation, within the tolerance.
        let mut s = ok.clone();
        s.by_kind.insert(PlantKind::Coal, Megawatts::new(94.0));
        assert!(!conserves(&s));
        s.by_kind
            .insert(PlantKind::Coal, Megawatts::new(95.0 + 5e-7));
        assert!(conserves(&s));

        // 0 ≤ served, inclusive of the tolerance.
        assert!(conserves(&summary(0.0, -t, 0.0, t, &[])));
        assert!(!conserves(&summary(0.0, -2.0 * t, 0.0, 2.0 * t, &[])));

        // served ≤ demand, inclusive of the tolerance (2t − t = t exactly).
        assert!(conserves(&summary(t, 2.0 * t, 2.0 * t, 0.0, &[2.0 * t])));
        assert!(!conserves(&summary(t, 3.0 * t, 3.0 * t, 0.0, &[3.0 * t])));

        // curtailed ≥ 0, inclusive of the tolerance.
        let mut s = ok.clone();
        s.curtailed = Megawatts::new(-t);
        assert!(conserves(&s));
        s.curtailed = Megawatts::new(-2.0 * t);
        assert!(!conserves(&s));
    }

    #[test]
    fn planned_outage_energy_cannot_exceed_unserved() {
        // Demand 100 MW, a 20% outage and 5% protection shed of the 80 MW
        // load: served 76, unserved 24, of which 20 is planned.
        let mut ok = summary(100.0, 76.0, 76.0, 0.0, &[76.0]);
        ok.outage = Ratio::new(0.2).unwrap();
        assert!(conserves(&ok));
        // The planned cut is part of unserved: a summary whose unserved is
        // below demand × outage (beyond float noise) is inconsistent.
        let mut broken = ok.clone();
        broken.served = Megawatts::new(81.0);
        broken.unserved = Megawatts::new(19.0);
        broken.generation = Megawatts::new(81.0);
        broken.by_plant = BTreeMap::from([(PlantId(0), Megawatts::new(81.0))]);
        broken.by_kind = BTreeMap::from([(PlantKind::Coal, Megawatts::new(81.0))]);
        assert!(!conserves(&broken));
        // Float noise below the 1e-6 × demand tolerance is fine.
        broken.unserved = Megawatts::new(20.0 - 5e-5);
        broken.served = Megawatts::new(80.0 + 5e-5);
        broken.generation = broken.served;
        broken.by_plant = BTreeMap::from([(PlantId(0), broken.served)]);
        broken.by_kind = BTreeMap::from([(PlantKind::Coal, broken.served)]);
        assert!(conserves(&broken));
    }

    #[test]
    fn a_broken_last_tick_is_reported_with_its_tick() {
        let mut s = session_with(|_| {});
        s.begin_operating().unwrap();
        s.tick().unwrap();
        assert_eq!(s.check_invariants(), Ok(()));
        let last = s.day_ticks.last_mut().unwrap();
        last.imbalance += Megawatts::new(1.0);
        assert_eq!(
            s.check_invariants(),
            Err(InvariantViolation::ConservationBroken { tick: 0 })
        );
    }

    /// One tick into the first day with the inherited schedule undone, so
    /// the start fleet is offline and reported at 0 MW.
    fn operating_after_one_tick(
        edit: impl FnOnce(&mut crate::region::RegionDefinitionParts),
    ) -> GameSession {
        let mut s = session_with(edit);
        s.empty_start();
        s.begin_operating().unwrap();
        s.tick().unwrap();
        assert_eq!(s.check_invariants(), Ok(()));
        s
    }

    #[test]
    fn the_last_tick_must_report_every_live_plant_output() {
        // The start fleet is uncommitted, so coal reported 0 MW.
        let mut s = operating_after_one_tick(|_| {});
        let coal = plant_id(&s, PlantKind::Coal);
        set_output(&mut s, coal, 300.0);
        assert_eq!(
            s.check_invariants(),
            Err(InvariantViolation::GenerationMismatch { tick: 0 })
        );

        // A connected plant missing from the summary is a mismatch too.
        let mut s = operating_after_one_tick(|_| {});
        let coal = plant_id(&s, PlantKind::Coal);
        s.day_ticks.last_mut().unwrap().by_plant.remove(&coal);
        assert_eq!(
            s.check_invariants(),
            Err(InvariantViolation::GenerationMismatch { tick: 0 })
        );
    }

    #[test]
    fn only_connected_commissioned_plants_count_as_live() {
        let mut s = session_with(|_| {});
        let (built, _) = s
            .build_plant(PlantKind::Peaker, CellCoord { x: 4, y: 4 })
            .unwrap();
        let (unlinked, _) = s
            .build_plant(PlantKind::Peaker, CellCoord { x: 5, y: 5 })
            .unwrap();
        s.begin_operating().unwrap();
        s.tick().unwrap();
        // Under construction but connected: still not live.
        let node = s.power.plant(built).unwrap().node();
        s.power
            .network_mut()
            .add_line(LineId(900), node, s.anchor, LineStatus::InService)
            .unwrap();
        assert_eq!(s.check_invariants(), Ok(()));
        // Commissioned (online) but unconnected: not live either.
        set_output(&mut s, unlinked, 100.0);
        assert_eq!(s.check_invariants(), Ok(()));
    }

    #[test]
    fn a_zero_length_manual_start_after_the_tick_is_not_a_mismatch() {
        let instant = |parts: &mut crate::region::RegionDefinitionParts| {
            for spec in &mut parts.catalog {
                if spec.kind == PlantKind::Peaker {
                    spec.startup = SimDuration::from_ticks(0);
                }
            }
        };
        let mut s = operating_after_one_tick(instant);
        let peaker = plant_id(&s, PlantKind::Peaker);
        // Offline at 0 MW in the summary, online at minimum now.
        s.start_plant(peaker).unwrap();
        assert!(s.power.plant(peaker).unwrap().is_online());
        assert_eq!(s.check_invariants(), Ok(()));
        // Once the summary reports it running, its output must match again.
        s.tick().unwrap();
        assert_eq!(s.check_invariants(), Ok(()));
        set_output(&mut s, peaker, 100.0);
        assert_eq!(
            s.check_invariants(),
            Err(InvariantViolation::GenerationMismatch { tick: 1 })
        );
    }
}

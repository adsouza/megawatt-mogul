//! Deterministic, loss-aware dispatch across the automatic DC network.
use crate::ids::{LinkId, NodeId, PlantId};
use std::collections::{BTreeMap, BTreeSet};

const EPS: f64 = 1e-7;

#[derive(Debug, Clone)]
struct Offer {
    plant: Option<PlantId>,
    cost: f64,
    capacity: f64,
    used: f64,
    /// Extra variable output released from existing curtailment.
    curtailed: bool,
}
#[derive(Debug, Clone, Default)]
struct Bus {
    /// Demand after fixed generation and fixed transfers; negative means surplus.
    need: f64,
    offers: Vec<Offer>,
    export_blocked: bool,
}
#[derive(Debug, Clone)]
struct Edge {
    id: LinkId,
    from: NodeId,
    to: NodeId,
    efficiency: f64,
    lower: f64,
    upper: f64,
    flow: f64,
}

/// A path uses sending-end MW at each edge. Losses compound along the route.
#[derive(Clone)]
struct Path {
    steps: Vec<(usize, f64)>,
    efficiency: f64,
    capacity: f64,
}
fn best_path(source: NodeId, destination: NodeId, edges: &[Edge]) -> Option<Path> {
    let mut best = BTreeMap::from([(source, (1.0f64, Vec::new(), f64::INFINITY))]);
    let mut visited = BTreeSet::new();
    loop {
        let next = best
            .iter()
            .filter(|(id, _)| !visited.contains(*id))
            .max_by(|(a, x), (b, y)| x.0.total_cmp(&y.0).then_with(|| b.cmp(a)))
            .map(|(id, state)| (*id, state.clone()));
        let (id, (efficiency, steps, capacity)) = next?;
        if id == destination {
            return Some(Path {
                steps,
                efficiency,
                capacity,
            });
        }
        visited.insert(id);
        for (index, edge) in edges.iter().enumerate() {
            let (to, sign, room) = if edge.from == id && edge.flow >= -EPS {
                (edge.to, 1.0, edge.upper - edge.flow)
            } else if edge.to == id && edge.flow <= EPS {
                (edge.from, -1.0, edge.flow - edge.lower)
            } else {
                continue;
            };
            if room <= EPS || visited.contains(&to) {
                continue;
            }
            let candidate = efficiency * edge.efficiency;
            if best.get(&to).is_none_or(|x| candidate > x.0 + EPS) {
                let mut route = steps.clone();
                route.push((index, sign));
                best.insert(to, (candidate, route, capacity.min(room / efficiency)));
            }
        }
    }
}

struct Allocation {
    source: NodeId,
    offer: usize,
    destination: NodeId,
    sink: Option<usize>,
    path: Path,
    sent: f64,
    score: f64,
}

/// Reserve local load first, then cover shortages, then replace expensive flexible output.
/// This is a deterministic merit-order heuristic, rather than an optimal-power-flow solver.
fn dispatch(buses: &mut BTreeMap<NodeId, Bus>, edges: &mut [Edge]) {
    for bus in buses.values_mut() {
        bus.offers.sort_by(|a, b| {
            a.cost
                .total_cmp(&b.cost)
                .then_with(|| a.plant.cmp(&b.plant))
        });
        for offer in &mut bus.offers {
            offer.used = bus.need.max(0.0).min(offer.capacity);
            bus.need -= offer.used;
        }
        if bus.need < -EPS {
            bus.offers.insert(
                0,
                Offer {
                    plant: None,
                    cost: 0.0,
                    capacity: -bus.need,
                    used: 0.0,
                    curtailed: false,
                },
            );
            bus.need = 0.0;
        }
    }
    for economic in [false, true] {
        loop {
            // Stable traversal supplies the final tie-break without depending on hash order.
            let mut chosen: Option<Allocation> = None;
            for (&source, bus) in buses.iter() {
                if bus.export_blocked {
                    continue;
                }
                for (offer_index, offer) in bus.offers.iter().enumerate() {
                    if offer.capacity - offer.used <= EPS {
                        continue;
                    }
                    for (&destination, receiver) in buses.iter() {
                        if source == destination {
                            continue;
                        }
                        let sink = if economic {
                            receiver
                                .offers
                                .iter()
                                .enumerate()
                                .rev()
                                .find(|(_, o)| o.used > EPS && o.cost > offer.cost + EPS)
                                .map(|(i, o)| (Some(i), o.used, o.cost))
                        } else {
                            (receiver.need > EPS).then_some((None, receiver.need, 0.0))
                        };
                        let Some((sink_index, need, avoided_cost)) = sink else {
                            continue;
                        };
                        let Some(path) = best_path(source, destination, edges) else {
                            continue;
                        };
                        let delivered_cost = offer.cost / path.efficiency;
                        let score = if economic {
                            delivered_cost - avoided_cost
                        } else {
                            delivered_cost
                        };
                        if economic && score >= -EPS {
                            continue;
                        }
                        let sent = (offer.capacity - offer.used)
                            .min(path.capacity)
                            .min(need / path.efficiency);
                        if sent <= EPS {
                            continue;
                        }
                        if chosen.as_ref().is_none_or(|x| score < x.score - EPS) {
                            chosen = Some(Allocation {
                                source,
                                offer: offer_index,
                                destination,
                                sink: sink_index,
                                path,
                                sent,
                                score,
                            });
                        }
                    }
                }
            }
            let Some(Allocation {
                source,
                offer,
                destination,
                sink,
                path,
                sent,
                ..
            }) = chosen
            else {
                break;
            };
            buses.get_mut(&source).expect("source").offers[offer].used += sent;
            let receiver = buses.get_mut(&destination).expect("destination");
            let delivered = sent * path.efficiency;
            if let Some(sink) = sink {
                receiver.offers[sink].used -= delivered;
            } else {
                receiver.need -= delivered;
            }
            let mut power = sent;
            for (index, sign) in path.steps {
                edges[index].flow += sign * power;
                power *= edges[index].efficiency;
            }
        }
    }
}

use super::{PowerSystem, is_commissioned};
use crate::{
    grid::LineStatus,
    region::{PlantKind, RegionDefinition},
    time::{SimMonth, TICKS_PER_HOUR},
    units::{Hertz, Megawatts},
};

fn inject(buses: &mut BTreeMap<NodeId, Bus>, from: NodeId, to: NodeId, flow: f64, efficiency: f64) {
    let (sender, receiver) = if flow >= 0.0 { (from, to) } else { (to, from) };
    buses.get_mut(&sender).expect("sender").need += flow.abs();
    buses.get_mut(&receiver).expect("receiver").need -= flow.abs() * efficiency;
}

impl PowerSystem {
    /// Pure, shared hourly plan. Never changes water, storage, random draws or manual schedules.
    pub fn with_transfer_plan(&self, region: &RegionDefinition, month: SimMonth) -> Self {
        let mut planned = self.clone();
        planned.refresh_transfer_plan(region, month);
        planned
    }

    pub(super) fn refresh_transfer_plan(&mut self, region: &RegionDefinition, month: SimMonth) {
        self.transfer_plan_from(region, month, 0);
    }
    pub(super) fn refresh_live_transfer_plan(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
    ) {
        self.transfer_plan_from(region, month, tick);
    }
    fn transfer_plan_from(&mut self, region: &RegionDefinition, month: SimMonth, start: u32) {
        let first_hour = (start / TICKS_PER_HOUR) as usize;
        if !self.network.links().any(|l| l.auto_follow) {
            return;
        }
        let Some(spec) = region.hvdc() else {
            return;
        };
        // Base forecasts include fixed manual transfers, but no hypothetical automatic imports.
        for link in self.network.links_mut().filter(|l| l.auto_follow) {
            link.automatic_schedule[first_hour..].fill(0.0);
        }
        let mut hours: Vec<BTreeMap<NodeId, Bus>> = (0..24).map(|_| BTreeMap::new()).collect();
        for &id in self.island_nodes.keys() {
            let mut local = self.scoped_island(id, region).expect("island");
            local.wind_factor = self.wind_factor;
            local.wind_tick = self.wind_tick;
            local.wind_month = self.wind_month;
            let hydro = if start == 0 {
                local.preview_hydro(region, month)
            } else {
                local.plan_hydro(region, month, first_hour as u8);
                local.compute_hydro_plan(region, month, first_hour as u8, |p| {
                    p.hydro_budget()
                        .unwrap_or(crate::units::MegawattHours::ZERO)
                })
            };
            let forecast = if start == 0 {
                local.supply_forecast(region, month).minutes
            } else {
                local.project_storage_supply(
                    region,
                    month,
                    start,
                    crate::time::TICKS_PER_DAY,
                    false,
                    &mut local.battery_plan.clone(),
                )
            };
            for (h, buses) in hours.iter_mut().enumerate().skip(first_hour) {
                let mut bus = Bus::default();
                let mut flexible = 0.0;
                for p in local.connected_plants().filter(|p| {
                    is_commissioned(p)
                        && p.kind().is_dispatchable()
                        && p.auto_follow()
                        && p.commitment().is_committed(h as u8)
                }) {
                    let cap = if p.kind() == PlantKind::Hydro {
                        hydro.get(&p.id()).map_or(0.0, |f| f.hourly[h].value())
                    } else {
                        p.available_capacity().value()
                    };
                    let cap = if start > 0 {
                        (h as u32 * 60..(h as u32 + 1) * 60)
                            .map(|tick| local.deliverable(p, start, tick - start).value())
                            .fold(cap, f64::min)
                    } else {
                        cap
                    };
                    let floor = p.min_output().value().min(cap);
                    flexible += cap;
                    bus.need -= floor;
                    bus.offers.push(Offer {
                        plant: Some(p.id()),
                        cost: p.spec().marginal_cost.value(),
                        capacity: (cap - floor).max(0.0),
                        used: 0.0,
                        curtailed: false,
                    });
                }
                // Reserve the worst minute's local requirement. Thermal offers are constant
                // within an hour; water-aware hydro and storage come from the shared evaluator.
                let residual = forecast[h * 60 - start as usize..(h + 1) * 60 - start as usize]
                    .iter()
                    .map(|p| p.demand.value() - p.net_supply().value() + flexible)
                    .fold(f64::NEG_INFINITY, f64::max);
                bus.need += residual;
                buses.insert(id, bus);
            }
        }
        for (hour, buses) in hours.iter_mut().enumerate().skip(first_hour) {
            let mut edges = Vec::new();
            for link in self
                .network
                .links()
                .filter(|l| l.auto_follow && l.override_mw.is_none())
            {
                let plan = self.link_planning_forecast(link.id).expect("link");
                let (Some(from), Some(to)) = (plan.from_island, plan.to_island) else {
                    continue;
                };
                if !plan.available(hour) || !self.island_active(from) || !self.island_active(to) {
                    continue;
                }
                let efficiency = 1.0
                    - 2.0 * spec.converter_loss
                    - spec.loss_per_cell * crate::region::whole_cells(link.length_cells) as f64;
                edges.push(Edge {
                    id: link.id,
                    from,
                    to,
                    efficiency,
                    lower: -spec.rating_mw,
                    upper: spec.rating_mw,
                    flow: 0.0,
                });
            }
            dispatch(buses, &mut edges);
            for edge in edges {
                self.network
                    .link_mut(edge.id)
                    .expect("link")
                    .automatic_schedule[hour] = edge.flow;
            }
        }
    }

    /// Choose this minute's *reachable* dispatch and transfers before stepping any island.
    /// Floors include shutdown tails and one-tick downward ramps. Storage is local: the
    /// allocator never offers its nameplate MW or future charge as exportable generation.
    pub(super) fn dispatch_transfers(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
        error: f64,
        terminals: &BTreeMap<NodeId, (bool, Hertz)>,
    ) {
        if !self
            .network
            .links()
            .any(|l| l.auto_follow && l.override_mw.is_none() && l.status == LineStatus::InService)
        {
            return;
        }
        let Some(spec) = region.hvdc() else {
            return;
        };
        let membership: BTreeMap<_, _> = self
            .island_nodes
            .iter()
            .flat_map(|(id, nodes)| nodes.iter().map(|n| (*n, *id)))
            .collect();
        let mut buses = BTreeMap::new();
        let mut floors = BTreeMap::new();
        let mut storage_grids = BTreeMap::new();
        for &id in self.island_nodes.keys() {
            let mut local = self.scoped_island(id, region).expect("island");
            local.clearness = self.clearness;
            local.wind_factor = self.wind_factor;
            local.wind_tick = self.wind_tick;
            local.wind_month = self.wind_month;
            if tick > 0 && tick.is_multiple_of(TICKS_PER_HOUR) {
                local.plan_hydro(region, month, (tick / TICKS_PER_HOUR) as u8);
            }
            let (available, _, withheld) = local.availability(region, month, tick);
            local.apply_commitment(tick, &mut Vec::new());
            let ceilings = local.agc_ceilings(tick);
            let demand = (local.connected_demand_forecast(region, month, tick).value()
                * (1.0 + error))
                .max(0.0);
            let restoring = if local.outage > local.outage_target {
                local
                    .outage_target
                    .value()
                    .max(local.outage.value() - region.frequency().shed_step.value())
            } else {
                local.outage.value()
            };
            let served = demand * (1.0 - restoring) * (1.0 - local.protection.shed.value());
            let mut bus = Bus {
                need: served,
                offers: Vec::new(),
                export_blocked: local.outage.value() > 0.0 || local.protection.shed.value() > 0.0,
            };
            if withheld.value() > EPS && !terminals[&id].0 {
                bus.offers.push(Offer {
                    plant: None,
                    cost: 0.0,
                    capacity: withheld.value(),
                    used: 0.0,
                    curtailed: true,
                });
            }
            // Request one restoration step ahead. A grid with unserved customers cannot
            // export spare capacity; local protection still settles the actual restoration.
            for p in local.connected_plants() {
                if p.kind() == PlantKind::Battery {
                    continue;
                }
                let cap = super::agc_cap(&available, &ceilings, p.id());
                let mut low = p.clone();
                let flexible = p.kind().is_dispatchable()
                    && p.auto_follow()
                    && p.is_online()
                    && !terminals[&id].0;
                if flexible {
                    let _ = low.set_target(p.min_output());
                }
                low.step(available[&p.id()]);
                bus.need -= low.output().value();
                if flexible {
                    let mut high = p.clone();
                    let _ = high.set_target(cap.clamp(p.min_output(), p.available_capacity()));
                    high.step(available[&p.id()]);
                    let floor = low.output().value();
                    floors.insert(p.id(), floor);
                    bus.offers.push(Offer {
                        plant: Some(p.id()),
                        cost: p.spec().marginal_cost.value(),
                        capacity: (high.output().value() - floor).max(0.0),
                        used: 0.0,
                        curtailed: false,
                    });
                }
            }
            if local.has_batteries() {
                storage_grids.insert(id, (local, demand, served));
            }
            buses.insert(id, bus);
        }
        let mut edges = Vec::new();
        for link in self.network.links() {
            if link.status != LineStatus::InService {
                continue;
            }
            let (Some(&from), Some(&to)) = (
                membership.get(&link.from_grid()),
                membership.get(&link.to_grid()),
            ) else {
                continue;
            };
            let efficiency = 1.0
                - 2.0 * spec.converter_loss
                - spec.loss_per_cell * crate::region::whole_cells(link.length_cells) as f64;
            let automatic = link.auto_follow && link.override_mw.is_none();
            let mut baseline = link.clone();
            let intended = if automatic {
                0.0
            } else {
                link.target((tick / 60) as usize)
            };
            let flow = super::islands::next_link_flow(
                &mut baseline,
                region,
                intended,
                terminals[&from],
                terminals[&to],
            );
            inject(&mut buses, from, to, flow, efficiency);
            if automatic && !baseline.blocked && !baseline.runback {
                let old = link.flow.value();
                let mut lower = (-spec.rating_mw).max(old - spec.ramp_mw_per_min) - flow;
                let mut upper = spec.rating_mw.min(old + spec.ramp_mw_per_min) - flow;
                // A mandatory ramp-down tail cannot be reversed in this tick. Never start
                // an export from a weak terminal, even when the previous flow was zero.
                if flow > EPS || terminals[&to].1 < region.frequency().shed_below {
                    lower = 0.0;
                }
                if flow < -EPS || terminals[&from].1 < region.frequency().shed_below {
                    upper = 0.0;
                }
                edges.push(Edge {
                    id: link.id,
                    from,
                    to,
                    efficiency,
                    lower,
                    upper,
                    flow: 0.0,
                });
            }
        }
        dispatch(&mut buses, &mut edges);
        for (id, bus) in &buses {
            let released: f64 = bus
                .offers
                .iter()
                .filter(|o| o.curtailed)
                .map(|o| o.used)
                .sum();
            if released > 0.0
                && let Some(state) = self.island_states.get_mut(id)
            {
                state.release_curtailment(Megawatts::new(released));
            }
        }
        // The minimum unavoidable flow was already charged to the buses above.
        for link in self
            .network
            .links_mut()
            .filter(|l| l.auto_follow && l.override_mw.is_none())
        {
            let (Some(&from), Some(&to)) = (
                membership.get(&link.from_grid()),
                membership.get(&link.to_grid()),
            ) else {
                link.automatic_target = 0.0;
                continue;
            };
            let mut baseline = link.clone();
            let tail = super::islands::next_link_flow(
                &mut baseline,
                region,
                0.0,
                terminals[&from],
                terminals[&to],
            );
            link.automatic_target = tail
                + edges
                    .iter()
                    .find(|e| e.id == link.id)
                    .map_or(0.0, |e| e.flow);
        }
        // Charging uses the projected physical dispatch *after transfers*, including
        // battery inventory after absorbing unavoidable generation tails. The original
        // local taper must see exports as load, not as spare charging headroom.
        if buses.values().all(|b| b.need <= EPS) && !storage_grids.is_empty() {
            let mut injections = BTreeMap::<NodeId, f64>::new();
            for link in self
                .network
                .links()
                .filter(|l| l.status == LineStatus::InService)
            {
                let (Some(&from), Some(&to)) = (
                    membership.get(&link.from_grid()),
                    membership.get(&link.to_grid()),
                ) else {
                    continue;
                };
                let mut projected = link.clone();
                let intended = link.target((tick / 60) as usize);
                let flow = super::islands::next_link_flow(
                    &mut projected,
                    region,
                    intended,
                    terminals[&from],
                    terminals[&to],
                );
                let efficiency = 1.0
                    - 2.0 * spec.converter_loss
                    - spec.loss_per_cell * crate::region::whole_cells(link.length_cells) as f64;
                let (sender, receiver) = if flow >= 0.0 { (from, to) } else { (to, from) };
                *injections.entry(sender).or_default() -= flow.abs();
                *injections.entry(receiver).or_default() += flow.abs() * efficiency;
            }
            for (id, (mut local, demand, served)) in storage_grids {
                let bus = buses.get_mut(&id).expect("storage grid");
                let released: f64 = bus
                    .offers
                    .iter()
                    .filter(|o| o.curtailed)
                    .map(|o| o.used)
                    .sum();
                local.curtailment =
                    (local.curtailment - Megawatts::new(released)).max(Megawatts::ZERO);
                for offer in &bus.offers {
                    if let Some(plant) = offer.plant {
                        let p = local.plants.get_mut(&plant).expect("plant");
                        let target = Megawatts::new(floors[&plant] + offer.used)
                            .clamp(p.min_output(), p.available_capacity());
                        let _ = p.set_target(target);
                    }
                }
                let (available, _, _) = local.availability(region, month, tick);
                for (plant, p) in &mut local.plants {
                    p.step(available[plant]);
                }
                local.injection = injections.get(&id).copied().unwrap_or(0.0);
                let flows = local.propose_battery_flows(
                    region,
                    month,
                    tick,
                    Megawatts::new(demand),
                    Megawatts::new(served),
                );
                let request = local.battery_charge_request(
                    region,
                    month,
                    tick,
                    &flows,
                    Megawatts::new(served),
                );
                let surplus = (local.primary_supply().value() - served).max(0.0);
                let mut remaining = (request.value() - surplus).max(0.0);
                for offer in &mut bus.offers {
                    if offer
                        .plant
                        .is_none_or(|plant| self.plants[&plant].kind() == PlantKind::Hydro)
                    {
                        continue;
                    }
                    let extra = remaining.min((offer.capacity - offer.used).max(0.0));
                    offer.used += extra;
                    remaining -= extra;
                }
            }
        }
        for bus in buses.values() {
            for offer in &bus.offers {
                if let Some(id) = offer.plant {
                    let p = self.plants.get_mut(&id).expect("plant");
                    let target = Megawatts::new(floors[&id] + offer.used)
                        .clamp(p.min_output(), p.available_capacity());
                    let _ = p.set_target(target);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn offer(cost: f64, capacity: f64) -> Offer {
        Offer {
            plant: None,
            cost,
            capacity,
            used: 0.0,
            curtailed: false,
        }
    }
    fn bus(need: f64, offers: Vec<Offer>) -> Bus {
        Bus {
            need,
            offers,
            export_blocked: false,
        }
    }
    fn edge(id: u32, from: u32, to: u32, rating: f64, efficiency: f64) -> Edge {
        Edge {
            id: LinkId(id),
            from: NodeId(from),
            to: NodeId(to),
            efficiency,
            lower: -rating,
            upper: rating,
            flow: 0.0,
        }
    }
    #[test]
    fn transfers_cover_shortage_and_pay_for_losses_without_spending_surplus_twice() {
        let mut buses = BTreeMap::from([
            (NodeId(0), bus(100.0, vec![offer(10.0, 200.0)])),
            (NodeId(1), bus(90.0, vec![])),
            (NodeId(2), bus(90.0, vec![])),
        ]);
        let mut edges = vec![edge(0, 0, 1, 200.0, 0.9), edge(1, 0, 2, 200.0, 0.9)];
        dispatch(&mut buses, &mut edges);
        assert!((edges.iter().map(|e| e.flow).sum::<f64>() - 100.0).abs() < EPS);
        assert!((buses.values().map(|b| b.need.max(0.0)).sum::<f64>() - 90.0).abs() < EPS);
    }
    #[test]
    fn cheap_generation_replaces_expensive_generation_only_when_losses_allow_savings() {
        let mut buses = BTreeMap::from([
            (NodeId(0), bus(0.0, vec![offer(9.0, 200.0)])),
            (NodeId(1), bus(90.0, vec![offer(20.0, 100.0)])),
        ]);
        let mut edges = vec![edge(0, 0, 1, 200.0, 0.9)];
        dispatch(&mut buses, &mut edges);
        assert!((edges[0].flow - 100.0).abs() < EPS);
        assert!(buses[&NodeId(1)].offers[0].used.abs() < EPS);
        let mut expensive_losses = vec![edge(0, 0, 1, 200.0, 0.5)];
        let mut buses = BTreeMap::from([
            (NodeId(0), bus(0.0, vec![offer(11.0, 200.0)])),
            (NodeId(1), bus(90.0, vec![offer(20.0, 100.0)])),
        ]);
        dispatch(&mut buses, &mut expensive_losses);
        assert_eq!(expensive_losses[0].flow, 0.0);
    }
    #[test]
    fn paths_cross_multiple_links_and_reverse_stored_endpoint_order() {
        let mut buses = BTreeMap::from([
            (NodeId(0), bus(-100.0, vec![])),
            (NodeId(1), bus(0.0, vec![])),
            (NodeId(2), bus(81.0, vec![])),
        ]);
        let mut edges = vec![edge(0, 1, 0, 100.0, 0.9), edge(1, 1, 2, 100.0, 0.9)];
        dispatch(&mut buses, &mut edges);
        assert!((edges[0].flow + 100.0).abs() < EPS);
        assert!((edges[1].flow - 90.0).abs() < EPS);
        assert!(buses[&NodeId(2)].need.abs() < EPS);
    }
    proptest::proptest! {
        #[test]
        fn dispatch_conserves_energy_obeys_ratings_and_is_deterministic(
            needs in proptest::collection::vec(-100.0f64..500.0, 4),
            capacities in proptest::collection::vec(0.0f64..500.0, 4),
            costs in proptest::collection::vec(1.0f64..200.0, 4),
            rating in 1.0f64..300.0, efficiency in 0.5f64..1.0,
        ) {
            let mut buses: BTreeMap<_, _> = (0..4).map(|i| (NodeId(i as u32), bus(needs[i], vec![Offer { plant: Some(PlantId(i as u32)), cost: costs[i], capacity: capacities[i], used: 0.0, curtailed: false }]))).collect();
            let mut edges = vec![edge(0,0,1,rating,efficiency), edge(1,1,2,rating,efficiency), edge(2,2,3,rating,efficiency), edge(3,3,0,rating,efficiency), edge(4,0,2,rating,efficiency)];
            let mut again = buses.clone(); let mut repeated = edges.clone();
            dispatch(&mut buses, &mut edges); dispatch(&mut again, &mut repeated);
            for (a,b) in edges.iter().zip(&repeated) { proptest::prop_assert_eq!(a.flow,b.flow); proptest::prop_assert!(a.flow.abs() <= rating + EPS); }
            let supply = needs.iter().map(|n| (-n).max(0.0)).sum::<f64>() + buses.values().flat_map(|b| &b.offers).filter(|o| o.plant.is_some()).map(|o| o.used).sum::<f64>();
            let served = needs.iter().map(|n| n.max(0.0)).sum::<f64>() - buses.values().map(|b| b.need).sum::<f64>();
            let unused = buses.values().flat_map(|b| &b.offers).filter(|o| o.plant.is_none()).map(|o| o.capacity-o.used).sum::<f64>();
            let losses = edges.iter().map(|e| e.flow.abs()*(1.0-e.efficiency)).sum::<f64>();
            proptest::prop_assert!((supply-served-unused-losses).abs() < 1e-5);
            for b in buses.values() { for o in &b.offers { proptest::prop_assert!(o.used >= -EPS && o.used <= o.capacity + EPS); } }
        }
    }
}

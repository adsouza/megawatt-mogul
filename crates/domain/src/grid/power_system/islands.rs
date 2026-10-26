//! Separate copies of the existing single-bus controller, one per energised AC component.
use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct IslandState {
    noise: DemandNoise,
    clearness: f64,
    wind_factor: f64,
    wind_tick: Option<u32>,
    wind_month: Option<SimMonth>,
    battery_bus_surplus: Option<bool>,
    wind_needs_replan: bool,
    deviation: Hertz,
    band: FrequencyBand,
    protection: ProtectionState,
    curtailment: Megawatts,
    hydro_plan: BTreeMap<PlantId, PlantHydroPlan>,
    hydro_planned_from: u8,
    hydro_override: HydroOverride,
    outage: Ratio,
    outage_target: Ratio,
    battery_plan: BatteryScheduleView,
    battery_plan_dirty: bool,
    battery_tick: u32,
}
crate::state_record!(IslandState {
    noise,
    clearness,
    wind_factor,
    wind_tick,
    wind_month,
    battery_bus_surplus,
    wind_needs_replan,
    deviation,
    band,
    protection,
    curtailment,
    hydro_plan,
    hydro_planned_from,
    hydro_override,
    outage,
    outage_target,
    battery_plan,
    battery_plan_dirty,
    battery_tick
});
impl IslandState {
    fn capture(power: PowerSystem) -> Self {
        Self {
            noise: power.noise,
            clearness: power.clearness,
            wind_factor: power.wind_factor,
            wind_tick: power.wind_tick,
            wind_month: power.wind_month,
            battery_bus_surplus: power.battery_bus_surplus,
            wind_needs_replan: power.wind_needs_replan,
            deviation: power.deviation,
            band: power.band,
            protection: power.protection,
            curtailment: power.curtailment,
            hydro_plan: power.hydro_plan,
            hydro_planned_from: power.hydro_planned_from,
            hydro_override: power.hydro_override,
            outage: power.outage,
            outage_target: power.outage_target,
            battery_plan: power.battery_plan,
            battery_plan_dirty: power.battery_plan_dirty,
            battery_tick: power.battery_tick,
        }
    }
    fn apply(self, power: &mut PowerSystem) {
        power.noise = self.noise;
        power.clearness = self.clearness;
        power.wind_factor = self.wind_factor;
        power.wind_tick = self.wind_tick;
        power.wind_month = self.wind_month;
        power.battery_bus_surplus = self.battery_bus_surplus;
        power.wind_needs_replan = self.wind_needs_replan;
        power.deviation = self.deviation;
        power.band = self.band;
        power.protection = self.protection;
        power.curtailment = self.curtailment;
        power.hydro_plan = self.hydro_plan;
        power.hydro_planned_from = self.hydro_planned_from;
        power.hydro_override = self.hydro_override;
        power.outage = self.outage;
        power.outage_target = self.outage_target;
        power.battery_plan = self.battery_plan;
        power.battery_plan_dirty = self.battery_plan_dirty;
        power.battery_tick = self.battery_tick;
    }
    pub fn frequency(&self, region: &RegionDefinition) -> Hertz {
        region.frequency().nominal + self.deviation
    }
    pub fn mode(&self) -> GridMode {
        self.protection.mode
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IslandTickSummary {
    pub area: crate::region::AreaId,
    pub demand: Megawatts,
    pub served: Megawatts,
    pub unserved: Megawatts,
    pub generation: Megawatts,
    pub imports: Megawatts,
    pub exports: Megawatts,
    pub losses: Megawatts,
    pub frequency: Hertz,
    pub mode: GridMode,
    pub outage: Ratio,
    pub shed: Ratio,
    pub by_kind: BTreeMap<PlantKind, Megawatts>,
    pub incidents: Vec<crate::session::Incident>,
}
crate::state_record!(IslandTickSummary {
    area,
    demand,
    served,
    unserved,
    generation,
    imports,
    exports,
    losses,
    frequency,
    mode,
    outage,
    shed,
    by_kind,
    incidents
});

impl PowerSystem {
    /// Called only at a topology boundary. IDs are the smallest AC node IDs.
    pub fn recompute_islands(&mut self) {
        self.island_nodes = self.network.islands();
        self.island_states
            .retain(|id, _| self.island_nodes.contains_key(id));
    }
    pub(super) fn invalidate_island_plans(&mut self) {
        for state in self.island_states.values_mut() {
            state.battery_plan_dirty = true;
        }
    }
    pub(super) fn unplan_island_hydro(&mut self, id: PlantId) {
        for state in self.island_states.values_mut() {
            if let Some(plan) = state.hydro_plan.get_mut(&id) {
                plan.unplan();
            }
        }
    }
    pub(super) fn finish_island_hydro(&mut self, clear: bool) {
        if !clear {
            for link in self.network.links_mut() {
                link.override_mw = None;
            }
        }
        for state in self.island_states.values_mut() {
            if clear {
                state.hydro_plan.clear();
                state.hydro_planned_from = 0;
            } else {
                state.hydro_planned_from = 24;
            }
        }
    }
    pub(crate) fn wind_for_node(
        &self,
        node: NodeId,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
    ) -> (f64, (f64, f64, f64)) {
        let Some(state) = self
            .node_island(node)
            .and_then(|id| self.island_states.get(&id))
        else {
            return (
                self.wind_factor,
                self.wind_factor_forecast(region, month, tick),
            );
        };
        let w = region.wind();
        let mean = w.monthly[month.index0()];
        let factor = if state.wind_month == Some(month) {
            let horizon = tick.saturating_sub(state.wind_tick.unwrap_or(0));
            mean + w.persistence.powf(f64::from(horizon)) * (state.wind_factor - mean)
        } else {
            mean
        };
        (
            state.wind_factor,
            (
                factor,
                (factor - w.band.value()).max(0.0),
                (factor + w.band.value()).min(1.0),
            ),
        )
    }
    pub(crate) fn mode_at(&self, node: NodeId) -> GridMode {
        self.node_island(node)
            .and_then(|id| self.island_states.get(&id))
            .map_or(self.protection.mode, |s| s.protection.mode)
    }
    pub(crate) fn island_state_invalid(
        &self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
    ) -> bool {
        self.island_states.iter().any(|(id, s)| {
            let Some(nodes) = self.island_nodes.get(id) else {
                return true;
            };
            !(0.0..=1.0).contains(&s.wind_factor)
                || !(0.0..=1.0).contains(&s.clearness)
                || s.hydro_planned_from > 24
                || s.battery_tick > TICKS_PER_DAY
                || s.curtailment < Megawatts::ZERO
                || s.hydro_plan.keys().any(|id| {
                    self.plants
                        .get(id)
                        .is_none_or(|p| p.kind() != PlantKind::Hydro || !nodes.contains(&p.node()))
                })
                || s.battery_plan.plants.iter().any(|(id, plan)| {
                    self.plants.get(id).is_none_or(|p| {
                        p.kind() != PlantKind::Battery || !nodes.contains(&p.node())
                    }) || plan.charge.len() != TICKS_PER_DAY as usize
                        || plan.discharge.len() != TICKS_PER_DAY as usize
                })
                || self
                    .scoped_island(*id, region)
                    .is_some_and(|p| p.wind_state_invalid(month, tick))
        })
    }
    pub fn scheduled_injection(&self, tick: u32) -> Megawatts {
        Megawatts::new(self.injections[(tick / TICKS_PER_HOUR).min(23) as usize])
    }
    pub fn island_states(&self) -> &BTreeMap<NodeId, IslandState> {
        &self.island_states
    }
    pub fn island_nodes(&self) -> &BTreeMap<NodeId, BTreeSet<NodeId>> {
        &self.island_nodes
    }
    fn node_island(&self, node: NodeId) -> Option<NodeId> {
        self.island_nodes
            .iter()
            .find_map(|(id, nodes)| nodes.contains(&node).then_some(*id))
    }
    fn transfer_schedule(&self, id: NodeId, region: &RegionDefinition) -> [f64; 24] {
        let Some(spec) = region.hvdc() else {
            return [0.0; 24];
        };
        let mut injection = [0.0; 24];
        for link in self
            .network
            .links()
            .filter(|l| l.status == super::super::LineStatus::InService)
        {
            let from = self.node_island(link.from);
            let to = self.node_island(link.to);
            if from.is_none() || to.is_none() {
                continue;
            }
            let factor = 1.0
                - 2.0 * spec.converter_loss
                - spec.loss_per_cell * crate::region::whole_cells(link.length_cells) as f64;
            for (h, value) in injection.iter_mut().enumerate() {
                let flow = link.target(h);
                if from == Some(id) {
                    *value += if flow >= 0.0 { -flow } else { -flow * factor };
                }
                if to == Some(id) {
                    *value += if flow >= 0.0 { flow * factor } else { flow };
                }
            }
        }
        injection
    }
    /// A pure controller preview for one island, including scheduled transfers.
    pub fn scoped_island(&self, id: NodeId, region: &RegionDefinition) -> Option<Self> {
        let nodes = self.island_nodes.get(&id)?;
        let home = self.node_island(self.network.root());
        let belongs =
            |node| nodes.contains(&node) || (home == Some(id) && !self.network.is_connected(node));
        let mut network = self.network.clone();
        network.scope_connected(nodes.clone());
        let mut power = Self::new(
            network,
            self.plants
                .iter()
                .filter(|(_, p)| belongs(p.node()))
                .map(|(id, p)| (*id, p.clone()))
                .collect(),
            self.cities
                .iter()
                .filter(|(_, c)| belongs(c.node()))
                .map(|(id, c)| (*id, c.clone()))
                .collect(),
            self.model,
        );
        if let Some(state) = self.island_states.get(&id) {
            state.clone().apply(&mut power);
        }
        power.injections = self.transfer_schedule(id, region);
        Some(power)
    }
    pub(super) fn begin_islands(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        rng: &mut SimRng,
    ) -> DayInitialisation {
        self.recompute_islands();
        self.clearness = draw_clearness(region.solar(), month, rng);
        let wind = region.wind();
        self.wind_factor = (wind.monthly[month.index0()] + wind.initial_sd * rng.standard_normal())
            .clamp(0.0, 1.0);
        self.wind_tick = Some(0);
        self.wind_month = Some(month);
        self.noise = DemandNoise::default();

        for link in self.network.links_mut() {
            link.flow = Megawatts::ZERO;
            link.override_mw = None;
            link.blocked = false;
            link.runback = false;
            link.runback_terminal = None;
            link.loss = Megawatts::ZERO;
            if link.review_year != month.year() {
                link.sent = MegawattHours::ZERO;
                link.review_year = month.year();
            }
        }
        let mut island_outages = BTreeMap::new();
        let mut boundary_starts = BTreeMap::new();
        let mut outage = Ratio::ZERO;
        let ids: Vec<_> = self.island_nodes.keys().copied().collect();
        for id in ids {
            let mut local = self.scoped_island(id, region).expect("island exists");
            local.shared_error = Some(0.0);
            local.clearness = self.clearness;
            local.wind_factor = self.wind_factor;
            let init = local.begin_single_day(region, month, rng);
            if init.outage > Ratio::ZERO {
                island_outages.insert(id, init.outage);
            }
            boundary_starts.extend(init.boundary_starts);
            if init.outage > outage {
                outage = init.outage;
            }
            self.plants.extend(std::mem::take(&mut local.plants));
            self.island_states.insert(id, IslandState::capture(local));
        }
        DayInitialisation {
            island_outages,
            boundary_starts,
            outage,
        }
    }
    pub(super) fn tick_islands(
        &mut self,
        region: &RegionDefinition,
        month: SimMonth,
        tick: u32,
        rng: &mut SimRng,
    ) -> (TickSummary, Vec<DomainEvent>) {
        let error = self.draw_tick_weather(region, month, tick, rng);
        let mut transfers = BTreeMap::<NodeId, (f64, f64, f64)>::new();
        // Freeze terminal conditions before stepping any link or island this minute.
        let terminals: BTreeMap<_, _> = self
            .island_nodes
            .iter()
            .map(|(id, nodes)| {
                let state = self.island_states.get(id).expect("day initialised");
                let synchronous = self.plants.values().any(|p| {
                    nodes.contains(&p.node()) && p.kind().is_dispatchable() && p.is_online()
                });
                (
                    *id,
                    (
                        state.protection.mode == GridMode::Blackout || !synchronous,
                        state.deviation,
                    ),
                )
            })
            .collect();
        let node_islands: BTreeMap<_, _> = self
            .island_nodes
            .iter()
            .flat_map(|(id, nodes)| nodes.iter().map(|n| (*n, *id)))
            .collect();
        if let Some(spec) = region.hvdc() {
            for link in self.network.links_mut() {
                if link.status != super::super::LineStatus::InService {
                    continue;
                }
                let endpoints = node_islands.get(&link.from).zip(node_islands.get(&link.to));
                let Some((&from, &to)) = endpoints else {
                    link.flow = Megawatts::ZERO;
                    link.blocked = true;
                    continue;
                };
                link.blocked = terminals[&from].0 || terminals[&to].0;
                let old = link.flow.value();
                let intended = link.target((tick / TICKS_PER_HOUR) as usize);
                if link.runback_terminal.is_some_and(|terminal| {
                    terminals[&node_islands[&terminal]].1.value().abs()
                        <= region.frequency().normal_band.value()
                }) {
                    link.runback_terminal = None;
                }
                if link.runback_terminal.is_none() {
                    let next =
                        old + (intended - old).clamp(-spec.ramp_mw_per_min, spec.ramp_mw_per_min);
                    // Check the present sender first, then any sender a signed ramp would
                    // introduce. A reversal cannot start exporting from a weak terminal.
                    for flow in [old, next] {
                        let terminal = if flow < 0.0 || (flow == 0.0 && intended < 0.0) {
                            link.to
                        } else {
                            link.from
                        };
                        if terminals[&node_islands[&terminal]].1 < region.frequency().shed_below {
                            link.runback_terminal = Some(terminal);
                            break;
                        }
                    }
                }
                link.runback = link.runback_terminal.is_some();
                let target = if link.runback {
                    0.0
                } else {
                    link.target((tick / TICKS_PER_HOUR) as usize)
                };
                let flow = if link.blocked {
                    0.0
                } else {
                    old + (target - old).clamp(-spec.ramp_mw_per_min, spec.ramp_mw_per_min)
                };
                link.flow = Megawatts::new(flow);
                let loss = flow.abs()
                    * (2.0 * spec.converter_loss
                        + spec.loss_per_cell
                            * crate::region::whole_cells(link.length_cells) as f64);
                link.loss = Megawatts::new(loss);
                link.sent += Megawatts::new(flow.abs()).energy_over_ticks(1)
                    * f64::from(month.days_in_month());
                let (sender, receiver) = if flow >= 0.0 { (from, to) } else { (to, from) };
                transfers.entry(sender).or_default().1 += flow.abs();
                transfers.entry(sender).or_default().2 += loss;
                transfers.entry(receiver).or_default().0 += flow.abs() - loss;
            }
        }
        let mut events = Vec::new();
        let mut rows = BTreeMap::new();
        let mut total: Option<TickSummary> = None;
        for id in self.island_nodes.keys().copied().collect::<Vec<_>>() {
            let mut local = self.scoped_island(id, region).expect("island exists");
            let (imports, exports, losses) = transfers.get(&id).copied().unwrap_or_default();
            local.shared_error = Some(error);
            local.clearness = self.clearness;
            local.wind_factor = self.wind_factor;
            local.wind_tick = self.wind_tick;
            local.wind_month = self.wind_month;
            local.injection = imports - exports;
            local.transfer_exports = exports;
            let (summary, mut local_events) = local.tick_single(region, month, tick, rng);
            let incidents = local_events
                .iter()
                .filter_map(|e| {
                    use crate::session::{Incident, IncidentKind};
                    let kind = match e {
                        DomainEvent::Blackout => IncidentKind::Blackout,
                        DomainEvent::BlackoutRestored => IncidentKind::BlackoutRestored,
                        DomainEvent::LoadShed { mw } => IncidentKind::LoadShed { mw: *mw },
                        DomainEvent::OverFrequencyTrip { plant } => {
                            IncidentKind::OverFrequencyTrip { plant: *plant }
                        }
                        DomainEvent::RotatingOutageOrdered { share, .. }
                            if *share > Ratio::ZERO =>
                        {
                            IncidentKind::RotatingOutage { share: *share }
                        }
                        _ => return None,
                    };
                    Some(Incident { tick, kind })
                })
                .collect();
            rows.insert(
                id,
                IslandTickSummary {
                    area: region.area_at(self.network.node(id).expect("island root").cell),
                    demand: summary.demand,
                    served: summary.served,
                    unserved: summary.unserved,
                    generation: summary.generation,
                    imports: Megawatts::new(imports),
                    exports: Megawatts::new(exports),
                    losses: Megawatts::new(losses),
                    frequency: summary.frequency,
                    mode: summary.mode,
                    outage: summary.outage,
                    shed: summary.shed,
                    by_kind: summary.by_kind.clone(),
                    incidents,
                },
            );
            local_events.retain(|e| !matches!(e, DomainEvent::TickCompleted(_)));
            events.extend(local_events);
            self.plants.extend(std::mem::take(&mut local.plants));
            self.island_states.insert(id, IslandState::capture(local));
            if let Some(t) = total.as_mut() {
                t.demand += summary.demand;
                t.served += summary.served;
                t.unserved += summary.unserved;
                t.generation += summary.generation;
                t.battery_charge += summary.battery_charge;
                t.battery_discharge += summary.battery_discharge;
                t.curtailed += summary.curtailed;
                t.imbalance += summary.imbalance;
                t.batteries.extend(summary.batteries);
                t.by_plant.extend(summary.by_plant);
                for (k, v) in summary.by_kind {
                    *t.by_kind.entry(k).or_insert(Megawatts::ZERO) += v;
                }
                for (k, v) in summary.curtailed_by_kind {
                    *t.curtailed_by_kind.entry(k).or_insert(Megawatts::ZERO) += v;
                }
            } else {
                total = Some(summary);
            }
        }
        let mut total = total.expect("home island always exists");
        total.frequency = region.frequency().nominal;
        total.mode = GridMode::Normal;
        total.outage = Ratio::new(if total.demand > Megawatts::ZERO {
            rows.values()
                .map(|r| r.demand.value() * r.outage.value())
                .sum::<f64>()
                / total.demand.value()
        } else {
            0.0
        })
        .expect("weighted share");
        total.shed = Ratio::ZERO;
        total.islands = rows;
        events.push(DomainEvent::TickCompleted(total.clone()));
        (total, events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        generation::CommitmentSchedule,
        grid::{LineStatus, Node, NodeKind},
        ids::LinkId,
        region::fixtures,
    };
    fn pair() -> (PowerSystem, RegionDefinition, SimRng) {
        let region = RegionDefinition::try_from(fixtures::two_area_parts()).unwrap();
        let mut network = Network::new(
            NodeId(0),
            Node {
                kind: NodeKind::Plant(PlantId(2)),
                cell: CellCoord { x: 6, y: 4 },
            },
        );
        network
            .add_node(
                NodeId(1),
                Node {
                    kind: NodeKind::Plant(PlantId(3)),
                    cell: CellCoord { x: 3, y: 2 },
                },
            )
            .unwrap();
        network
            .add_link(LinkId(10), NodeId(0), NodeId(1), LineStatus::InService)
            .unwrap();
        let month = region.start().month;
        let plants = [
            (PlantId(2), NodeId(0), CellCoord { x: 6, y: 4 }),
            (PlantId(3), NodeId(1), CellCoord { x: 3, y: 2 }),
        ]
        .into_iter()
        .map(|(id, node, cell)| {
            let mut p = Plant::new_commissioned(
                id,
                node,
                cell,
                region.spec(PlantKind::Peaker).clone(),
                month,
            );
            p.set_commitment(CommitmentSchedule::all_day());
            p.set_auto_follow(true).unwrap();
            (id, p)
        })
        .collect();
        let mut power =
            PowerSystem::new(network, plants, BTreeMap::new(), NetworkModel::CopperPlate);
        power.network.link_mut(LinkId(10)).unwrap().schedule = [100.0; 24];
        let mut rng = SimRng::from_seed(7);
        power.begin_day(&region, month, &mut rng);
        (power, region, rng)
    }
    #[test]
    fn island_count_does_not_multiply_region_weather_or_demand_noise_draws() {
        let (mut power, region, mut rng) = pair();
        let mut expected = SimRng::from_seed(7);
        expected.standard_normal();
        expected.standard_normal();
        assert_eq!(rng, expected);
        power.tick(&region, region.start().month, 0, &mut rng);
        expected.standard_normal();
        assert_eq!(rng, expected);
        power.tick(&region, region.start().month, 1, &mut rng);
        expected.standard_normal();
        expected.standard_normal();
        assert_eq!(rng, expected);
    }
    #[test]
    fn runback_reads_previous_sender_state_never_reverses_and_waits_for_normal_band() {
        let (mut p, r, mut rng) = pair();
        let dc = p.network.link_mut(LinkId(10)).unwrap();
        dc.flow = Megawatts::new(80.0);
        dc.schedule = [-100.0; 24];
        p.island_states.get_mut(&NodeId(0)).unwrap().deviation = Hertz::new(-0.6);
        p.tick(&r, r.start().month, 0, &mut rng);
        assert_eq!(
            p.network.link(LinkId(10)).unwrap().flow,
            Megawatts::new(60.0)
        );
        assert!(p.network.link(LinkId(10)).unwrap().runback);
        p.island_states.get_mut(&NodeId(0)).unwrap().deviation = Hertz::new(0.2);
        p.tick(&r, r.start().month, 1, &mut rng);
        assert_eq!(
            p.network.link(LinkId(10)).unwrap().flow,
            Megawatts::new(40.0)
        );
        assert!(p.network.link(LinkId(10)).unwrap().runback);
        p.island_states.get_mut(&NodeId(0)).unwrap().deviation = Hertz::ZERO;
        p.tick(&r, r.start().month, 2, &mut rng);
        assert!(!p.network.link(LinkId(10)).unwrap().runback);
    }

    #[test]
    fn runback_remembers_its_sender_at_zero_flow_and_across_restore() {
        use crate::state::State;
        for reverse in [false, true] {
            let (mut p, r, mut rng) = pair();
            let direction = if reverse { -1.0 } else { 1.0 };
            let sender = if reverse { NodeId(1) } else { NodeId(0) };
            let dc = p.network.link_mut(LinkId(10)).unwrap();
            dc.flow = Megawatts::new(direction * 80.0);
            dc.schedule = [-direction * 100.0; 24];
            for tick in 0..5 {
                // Fix the previous-tick terminal conditions to isolate the runback state
                // machine from the frequency controller, in both transfer directions.
                for plant in p.plants.values_mut() {
                    plant.initialise_pattern_day();
                }
                for (id, state) in &mut p.island_states {
                    state.protection = ProtectionState::default();
                    state.deviation = if *id == sender {
                        Hertz::new(-0.6)
                    } else {
                        Hertz::ZERO
                    };
                }
                if tick == 4 {
                    assert_eq!(p.network.link(LinkId(10)).unwrap().flow, Megawatts::ZERO);
                    p = PowerSystem::from_state_bytes(&p.state_bytes()).unwrap();
                }
                p.tick(&r, r.start().month, tick, &mut rng);
            }
            let dc = p.network.link(LinkId(10)).unwrap();
            assert_eq!(dc.flow, Megawatts::ZERO);
            assert!(dc.runback);
            for state in p.island_states.values_mut() {
                state.deviation = Hertz::ZERO;
                state.protection = ProtectionState::default();
            }
            p.tick(&r, r.start().month, 5, &mut rng);
            let dc = p.network.link(LinkId(10)).unwrap();
            assert!(!dc.runback);
            assert_eq!(dc.flow, Megawatts::new(-direction * 20.0));
        }
    }

    #[test]
    fn disabling_hydro_agc_clears_the_island_plan_until_the_next_replan() {
        let (mut p, r, mut rng) = pair();
        let (id, node, cell) = (PlantId(5), NodeId(4), CellCoord { x: 10, y: 1 });
        p.network
            .add_node(
                node,
                Node {
                    kind: NodeKind::Plant(id),
                    cell,
                },
            )
            .unwrap();
        p.network
            .add_line(
                crate::ids::LineId(6),
                NodeId(0),
                node,
                LineStatus::InService,
            )
            .unwrap();
        let mut hydro = Plant::new_commissioned(
            id,
            node,
            cell,
            r.spec(PlantKind::Hydro).clone(),
            r.start().month,
        );
        hydro.set_commitment(CommitmentSchedule::all_day());
        hydro.set_auto_follow(true).unwrap();
        p.plants.insert(id, hydro);
        p.begin_day(&r, r.start().month, &mut rng);
        let before = p.scoped_island(NodeId(0), &r).unwrap();
        assert!(before.hydro_ceilings(1).contains_key(&id));
        let frozen = before.hydro_plan[&id].frozen;
        p.set_auto_follow(id, false).unwrap();
        let off = p.scoped_island(NodeId(0), &r).unwrap();
        assert_eq!(off.hydro_schedule().plants[&id].level, None);
        assert_eq!(off.hydro_plan[&id].frozen, frozen);
        p.set_auto_follow(id, true).unwrap();
        let mut on = p.scoped_island(NodeId(0), &r).unwrap();
        assert!(!on.hydro_ceilings(1).contains_key(&id));
        on.plan_hydro(&r, r.start().month, 1);
        assert!(on.hydro_ceilings(61).contains_key(&id));
    }

    #[test]
    fn runback_checks_the_new_sender_before_reversing() {
        for flow in [0.0, 10.0] {
            let (mut p, r, mut rng) = pair();
            let dc = p.network.link_mut(LinkId(10)).unwrap();
            dc.flow = Megawatts::new(flow);
            dc.runback = flow == 0.0;
            dc.runback_terminal = (flow == 0.0).then_some(NodeId(0));
            dc.schedule = [-100.0; 24];
            p.island_states.get_mut(&NodeId(0)).unwrap().deviation = Hertz::ZERO;
            p.island_states.get_mut(&NodeId(1)).unwrap().deviation = Hertz::new(-0.6);
            p.tick(&r, r.start().month, 0, &mut rng);
            let dc = p.network.link(LinkId(10)).unwrap();
            assert_eq!(dc.flow, Megawatts::ZERO);
            assert!(dc.runback);
            assert_eq!(dc.runback_terminal, Some(NodeId(1)));
        }
    }
    #[test]
    fn a_blacked_out_terminal_blocks_immediately_and_the_other_island_keeps_running() {
        let (mut p, r, mut rng) = pair();
        p.network.link_mut(LinkId(10)).unwrap().flow = Megawatts::new(100.0);
        p.island_states.get_mut(&NodeId(1)).unwrap().protection.mode = GridMode::Blackout;
        let (summary, _) = p.tick(&r, r.start().month, 0, &mut rng);
        assert!(p.network.link(LinkId(10)).unwrap().blocked);
        assert_eq!(p.network.link(LinkId(10)).unwrap().flow, Megawatts::ZERO);
        assert_eq!(summary.islands[&NodeId(0)].mode, GridMode::Normal);
        assert!(p.plant(PlantId(2)).unwrap().is_online());
    }
    proptest::proptest! {
        #[test]
        fn signed_transfers_conserve_energy_with_losses_and_ramp_bounds(target in -200.0f64..=200.0) {
            let(mut p,r,mut rng)=pair();p.network.link_mut(LinkId(10)).unwrap().schedule=[target;24];
            let(summary,_)=p.tick(&r,r.start().month,0,&mut rng);
            let link=p.network.link(LinkId(10)).unwrap();proptest::prop_assert!(link.flow.value().abs()<=20.0);
            let imports:f64=summary.islands.values().map(|s|s.imports.value()).sum();
            let exports:f64=summary.islands.values().map(|s|s.exports.value()).sum();
            let losses:f64=summary.islands.values().map(|s|s.losses.value()).sum();
            proptest::prop_assert!((exports-imports-losses).abs()<1e-9);
            proptest::prop_assert!((summary.generation.value()-summary.served.value()-summary.battery_charge.value()-summary.imbalance.value()-losses).abs()<1e-6);
        }
    }
}

//! The automatic commitment planner (spec 2026-10-31 §4): trims expensive thermal plants to the
//! day-ahead forecast plus a cushion. A pure query: every candidate is judged on a trial copy of
//! the session, so transfers, hydro and storage previews are re-planned exactly as the Planning
//! checks would re-plan them.

use std::collections::BTreeMap;

use super::GameSession;
use crate::generation::{CommitmentSchedule, Plant, PlantStatus};
use crate::ids::{NodeId, PlantId};
use crate::region::PlantKind;
use crate::time::{TICKS_PER_DAY, TICKS_PER_HOUR};

/// Commitment patterns by plant.
pub type CommitmentPlan = BTreeMap<PlantId, CommitmentSchedule>;

/// Megawatts and dollars below this are float noise, not a difference between plans.
const TOLERANCE: f64 = 1e-6;
/// Candidates fully evaluated per plant, best estimates first.
const FULL_EVALUATIONS: usize = 8;

/// One plan judged on the coming day.
struct Evaluation {
    /// Per local grid, per assessed minute: the cushioned requirement and the supply that can
    /// be counted on (dull weather, ramp-limited restarts, spare battery discharge).
    samples: BTreeMap<NodeId, BTreeMap<u32, (f64, f64)>>,
    /// Committed minimum generation above the net load, summed over the samples (MW).
    excess: f64,
    /// Fuel and start-ups on the forecast day, in dollars.
    cost: f64,
}

/// What each sample must keep: the requirement, or all the all-day fleet could deliver there.
struct Floor(BTreeMap<NodeId, BTreeMap<u32, f64>>);

impl Evaluation {
    fn floor(&self) -> Floor {
        Floor(
            self.samples
                .iter()
                .map(|(id, samples)| {
                    let floor = samples
                        .iter()
                        .map(|(tick, (required, supply))| (*tick, required.min(*supply)))
                        .collect();
                    (*id, floor)
                })
                .collect(),
        )
    }

    fn covers(&self, floor: &Floor) -> bool {
        floor.0.iter().all(|(id, ticks)| {
            ticks.iter().all(|(tick, floor)| {
                self.samples
                    .get(id)
                    .and_then(|samples| samples.get(tick))
                    .is_some_and(|(_, supply)| *supply >= floor - TOLERANCE)
            })
        })
    }

    /// Less overgeneration first, then lower cost (spec §4.4).
    fn better_than(&self, other: &Evaluation) -> bool {
        if (self.excess - other.excess).abs() > TOLERANCE {
            self.excess < other.excess
        } else {
            self.cost < other.cost - TOLERANCE
        }
    }

    /// Per local grid and hour, the least supply above the floor.
    fn hourly_slack(&self, floor: &Floor) -> BTreeMap<NodeId, [f64; 24]> {
        floor
            .0
            .iter()
            .map(|(id, ticks)| {
                let mut slack = [f64::INFINITY; 24];
                for (tick, floor) in ticks {
                    let supply = self.samples[id][tick].1;
                    let hour = (tick / TICKS_PER_HOUR) as usize;
                    slack[hour] = slack[hour].min(supply - floor);
                }
                (*id, slack)
            })
            .collect()
    }
}

/// What a plant restarting after an off-gap can deliver at `tick`: it comes online at minimum
/// output at the start of its committed run and climbs at its ramp rate. An all-day pattern
/// never restarts.
pub(super) fn restart_credit(
    hours: [bool; 24],
    minimum: f64,
    capacity: f64,
    ramp_per_tick: f64,
    tick: u32,
) -> f64 {
    let hour = (tick / TICKS_PER_HOUR) as usize;
    if hours.iter().all(|on| *on) || !hours[hour] {
        return capacity;
    }
    let mut start = hour;
    while hours[(start + 23) % 24] {
        start = (start + 23) % 24;
    }
    let since = (tick + TICKS_PER_DAY - start as u32 * TICKS_PER_HOUR) % TICKS_PER_DAY;
    (minimum + ramp_per_tick * f64::from(since)).min(capacity)
}

/// The number of restarts a pattern makes each day.
fn restarts(hours: [bool; 24]) -> u32 {
    (0..24)
        .filter(|&h| hours[h] && !hours[(h + 23) % 24])
        .count() as u32
}

/// Every pattern with one off-gap on the 24-hour circle, and all-off, that `valid` accepts.
fn candidates(valid: impl Fn([bool; 24]) -> bool) -> Vec<[bool; 24]> {
    let mut all = vec![[false; 24]];
    for start in 0..24 {
        for length in 1..24 {
            let mut hours = [true; 24];
            for offset in 0..length {
                hours[(start + offset) % 24] = false;
            }
            all.push(hours);
        }
    }
    all.into_iter().filter(|hours| valid(*hours)).collect()
}

fn trimmable(plant: &Plant, connected: bool) -> bool {
    connected
        && !matches!(plant.status(), PlantStatus::UnderConstruction { .. })
        && plant.auto_follow()
        && matches!(
            plant.kind(),
            PlantKind::Coal | PlantKind::Ccgt | PlantKind::Peaker
        )
}

impl GameSession {
    /// The commitment patterns that cover the coming day's forecast plus its cushion while
    /// burning the least expensive fuel (spec 2026-10-31 §4). Only connected, commissioned
    /// coal, CCGT and peaker plants on AGC are returned; everything else is left as it is.
    /// Pure: the session is unchanged.
    pub fn automatic_commitment_plan(&self) -> CommitmentPlan {
        let mut order: Vec<&Plant> = self
            .trimmable_ids()
            .into_iter()
            .filter_map(|id| self.power.plant(id))
            .collect();
        // Most expensive first: peakers, then CCGTs, then coal.
        order.sort_by(|a, b| {
            b.spec()
                .marginal_cost
                .value()
                .total_cmp(&a.spec().marginal_cost.value())
                .then(b.id().cmp(&a.id()))
        });
        let mut plan = self.all_day_plan();
        let mut best = self.evaluate(&plan);
        let floor = best.floor();
        for plant in order {
            let spec = plant.commitment_spec();
            let grid = self.grid_of(plant.node());
            let slack = best.hourly_slack(&floor);
            let slack = slack.get(&grid).copied().unwrap_or([f64::INFINITY; 24]);
            let credit = plant.available_capacity().value();
            let saving_per_hour = plant.min_output().value() * spec.marginal_cost.value();
            let startup = spec.startup_cost.cents() as f64 / 100.0;
            // Screen with the plant's own credit against the current slack, then judge the most
            // promising few on a full trial.
            let mut screened: Vec<(f64, [bool; 24])> =
                candidates(|hours| CommitmentSchedule::try_new(hours, &spec).is_ok())
                    .into_iter()
                    .filter(|hours| (0..24).all(|h| hours[h] || slack[h] >= credit - TOLERANCE))
                    .map(|hours| {
                        let off = hours.iter().filter(|on| !**on).count() as f64;
                        let estimate = off * saving_per_hour - f64::from(restarts(hours)) * startup;
                        (estimate, hours)
                    })
                    .collect();
            screened.sort_by(|a, b| b.0.total_cmp(&a.0));
            let mut chosen: Option<(CommitmentSchedule, Evaluation)> = None;
            for (_, hours) in screened.into_iter().take(FULL_EVALUATIONS) {
                let Ok(schedule) = CommitmentSchedule::try_new(hours, &spec) else {
                    continue;
                };
                let mut trial = plan.clone();
                trial.insert(plant.id(), schedule);
                let evaluation = self.evaluate(&trial);
                let beats =
                    evaluation.better_than(chosen.as_ref().map_or(&best, |(_, current)| current));
                if evaluation.covers(&floor) && beats {
                    chosen = Some((schedule, evaluation));
                }
            }
            if let Some((schedule, evaluation)) = chosen {
                plan.insert(plant.id(), schedule);
                best = evaluation;
            }
        }
        plan
    }

    /// Every trimmable plant committed all day (spec §4.3's reference plan).
    fn all_day_plan(&self) -> CommitmentPlan {
        self.trimmable_ids()
            .into_iter()
            .map(|id| (id, CommitmentSchedule::all_day()))
            .collect()
    }

    fn trimmable_ids(&self) -> Vec<PlantId> {
        self.plant_views()
            .into_iter()
            .filter(|view| {
                self.power
                    .plant(view.id)
                    .is_some_and(|plant| trimmable(plant, view.connected))
            })
            .map(|view| view.id)
            .collect()
    }

    /// The local grid a node belongs to: its synchronous island, or the anchor in a region
    /// planned as one system.
    fn grid_of(&self, node: NodeId) -> NodeId {
        if !self.planned_by_island() {
            return self.anchor;
        }
        self.power
            .island_nodes()
            .iter()
            .find_map(|(id, nodes)| nodes.contains(&node).then_some(*id))
            .unwrap_or(self.anchor)
    }

    fn planned_by_island(&self) -> bool {
        self.region.areas().is_some() && !self.power.island_nodes().is_empty()
    }

    /// The session with `plan`'s patterns applied, judged on every local grid.
    fn evaluate(&self, plan: &CommitmentPlan) -> Evaluation {
        let mut trial = self.clone();
        for (id, schedule) in plan {
            if let Some(plant) = trial.power.plant_mut(*id) {
                plant.set_commitment(*schedule);
            }
        }
        let mut evaluation = Evaluation {
            samples: BTreeMap::new(),
            excess: 0.0,
            cost: plan
                .values()
                .map(|schedule| schedule.hours())
                .zip(plan.keys())
                .filter_map(|(hours, id)| {
                    let plant = trial.power.plant(*id)?;
                    Some(f64::from(restarts(hours)) * plant.spec().startup_cost.cents() as f64)
                })
                .sum::<f64>()
                / 100.0,
        };
        if trial.planned_by_island() {
            // One transfer plan for every island, as each island's own view would compute it.
            let planned = trial.transfer_planned_power();
            for id in trial.power.island_nodes().keys() {
                if let Some(local) = trial.island_session_with(&planned, *id) {
                    local.judge_grid(*id, &mut evaluation);
                }
            }
        } else {
            let anchor = trial.anchor;
            trial.judge_grid(anchor, &mut evaluation);
        }
        evaluation
    }

    /// Adds this local grid's overgeneration and fuel (under the forecast weather), then its
    /// samples (under the forecast's dull, calm edge), to `evaluation`. Takes the grid by value
    /// and switches its weather in place: one copy per grid instead of two.
    fn judge_grid(mut self, id: NodeId, evaluation: &mut Evaluation) {
        let views = self.plant_views();
        evaluation.excess += self
            .minimum_generation_forecast()
            .iter()
            .map(|point| (point.committed_minimum - point.net_load).value().max(0.0))
            .sum::<f64>();
        evaluation.cost += self.forecast_fuel_cost(&views);

        let factor = 1.0 + 2.0 * self.region.demand_noise().forecast_error.value();
        let discharge_from = self
            .region
            .spec(PlantKind::Battery)
            .battery
            .map(|battery| u32::from(battery.discharge_start_hour) * TICKS_PER_HOUR);
        // (pattern, minimum, capacity, ramp per tick) of each trimmable plant that restarts.
        let restarting: Vec<([bool; 24], f64, f64, f64)> = views
            .iter()
            .filter_map(|view| {
                let plant = self.power.plant(view.id)?;
                let hours = plant.commitment().hours();
                (trimmable(plant, view.connected) && hours != [true; 24]).then(|| {
                    (
                        hours,
                        plant.min_output().value(),
                        plant.available_capacity().value(),
                        plant.spec().ramp.per_tick().value(),
                    )
                })
            })
            .collect();
        let dull = self.power.weather_forecast().low_edge(&self.region);
        self.power.set_weather_forecast(dull);
        let samples = self
            .supply_forecast()
            .assessed_minutes()
            .map(|(tick, point)| {
                let battery = match discharge_from {
                    Some(from) if tick >= from => {
                        let hours_left =
                            f64::from(TICKS_PER_DAY - tick) / f64::from(TICKS_PER_HOUR);
                        (point.battery_power - point.battery_discharge)
                            .value()
                            .max(0.0)
                            .min(point.battery_deliverable_energy.value() / hours_left)
                    }
                    _ => 0.0,
                };
                let restart_deficit: f64 = restarting
                    .iter()
                    .map(|&(hours, minimum, capacity, ramp)| {
                        capacity - restart_credit(hours, minimum, capacity, ramp, tick)
                    })
                    .sum();
                let supply = point.gross_supply.value() + battery - restart_deficit;
                (tick, (point.demand.value() * factor, supply))
            })
            .collect();
        evaluation.samples.insert(id, samples);
    }

    /// Fuel on the forecast day: each hour's net load, less manual targets and scheduled hydro,
    /// dispatched over the committed AGC plants from their minimums in `(marginal_cost, id)`
    /// order, like the boundary dispatch.
    fn forecast_fuel_cost(&self, views: &[super::views::PlantView]) -> f64 {
        let hydro = self.hydro_forecast();
        let mut cost = 0.0;
        for hour in 0..24u8 {
            let tick = u32::from(hour) * TICKS_PER_HOUR + TICKS_PER_HOUR / 2;
            let mut load = self.forecast_net_load(tick).value();
            let mut participants: Vec<&Plant> = Vec::new();
            for view in views.iter().filter(|view| view.connected) {
                let Some(plant) = self.power.plant(view.id) else {
                    continue;
                };
                if !plant.kind().is_dispatchable()
                    || matches!(plant.status(), PlantStatus::UnderConstruction { .. })
                    || !plant.commitment().is_committed(hour)
                {
                    continue;
                }
                let price = plant.spec().marginal_cost.value();
                if !plant.auto_follow() {
                    load -= plant.initial_target().value();
                    cost += plant.initial_target().value() * price;
                } else if plant.kind() == PlantKind::Hydro {
                    load -= hydro
                        .get(&plant.id())
                        .map_or(0.0, |hourly| hourly[usize::from(hour)].value());
                } else {
                    participants.push(plant);
                }
            }
            participants.sort_by(|a, b| {
                a.spec()
                    .marginal_cost
                    .value()
                    .total_cmp(&b.spec().marginal_cost.value())
                    .then(a.id().cmp(&b.id()))
            });
            let minimums: f64 = participants.iter().map(|p| p.min_output().value()).sum();
            let mut left = (load - minimums).max(0.0);
            for plant in participants {
                let minimum = plant.min_output().value();
                let extra = (plant.available_capacity().value() - minimum).min(left);
                left -= extra;
                cost += (minimum + extra) * plant.spec().marginal_cost.value();
            }
        }
        cost
    }
}

#[cfg(test)]
#[path = "auto_commit_tests.rs"]
mod tests;

//! The automatic commitment planner (spec 2026-10-31 §4): trims expensive thermal plants to the
//! day-ahead forecast plus a cushion. A pure query: every candidate is judged on a trial copy of
//! the session, so transfers, hydro and storage previews are re-planned exactly as the Planning
//! checks would re-plan them.

use std::collections::{BTreeMap, VecDeque};

use super::GameSession;
use crate::generation::{CommitmentSchedule, Plant, PlantStatus};
use crate::ids::{NodeId, PlantId};
use crate::region::PlantKind;
use crate::time::{TICKS_PER_DAY, TICKS_PER_HOUR};

/// Commitment patterns by plant.
pub type CommitmentPlan = BTreeMap<PlantId, CommitmentSchedule>;

/// Pure diagnostic data; formatting and recording belong to the outer layers.
#[derive(Debug, Clone, PartialEq)]
pub struct CommitmentReport {
    pub plan: CommitmentPlan,
    pub demand_cushion_factor: f64,
    pub score_tolerance: f64,
    pub evaluation_limit_per_plant: usize,
    pub reference_score: CommitmentScore,
    pub selected_score: CommitmentScore,
    /// In the planner's actual evaluation order, most expensive first.
    pub decisions: Vec<CommitmentDecision>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CommitmentScore {
    /// Forecast-day fuel plus recurring starts, in the region's currency units.
    pub estimated_cost: f64,
    /// Sum of sampled minimum-generation surplus (MW), not energy.
    pub excess: f64,
    /// Tightest assessed supply margin above the preserved all-day coverage floor.
    pub minimum_floor_margin_mw: Option<f64>,
    /// Tightest margin against the full cushioned demand, even if the fleet is inadequate.
    pub minimum_demand_margin_mw: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommitmentDecision {
    pub plant: PlantId,
    pub kind: PlantKind,
    pub grid: NodeId,
    pub before: CommitmentScore,
    pub selected: CommitmentSchedule,
    pub selected_score: CommitmentScore,
    pub valid_candidates: usize,
    pub capacity_screened: usize,
    pub evaluation_limited: usize,
    /// Conservative hourly capacity screen margin for switching this plant off all day.
    pub all_off_screen_margin_mw: f64,
    pub candidates: Vec<CommitmentCandidate>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommitmentCandidate {
    pub hours: [bool; 24],
    pub score: CommitmentScore,
    /// The incumbent at this point in the search, including any earlier accepted candidate.
    pub compared_to: CommitmentScore,
    pub covers_floor: bool,
    pub comparison: CommitmentComparison,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitmentComparison {
    LowerExcess,
    HigherExcess,
    LowerCost,
    HigherCost,
    EqualScore,
}

/// Megawatts and currency units below this are float noise, not a difference between plans.
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
    /// Fuel and start-ups on the forecast day, in the region's currency units.
    cost: f64,
}

/// What each sample must keep: the requirement, or all the all-day fleet could deliver there.
struct Floor(BTreeMap<NodeId, BTreeMap<u32, f64>>);

impl Evaluation {
    fn score(&self, floor: &Floor) -> CommitmentScore {
        CommitmentScore {
            estimated_cost: self.cost,
            excess: self.excess,
            minimum_floor_margin_mw: floor
                .0
                .iter()
                .flat_map(|(id, ticks)| {
                    ticks
                        .iter()
                        .map(|(tick, required)| self.samples[id][tick].1 - required)
                })
                .reduce(f64::min),
            minimum_demand_margin_mw: self
                .samples
                .values()
                .flat_map(|ticks| ticks.values().map(|(required, supply)| supply - required))
                .reduce(f64::min),
        }
    }

    fn comparison(&self, other: &Evaluation) -> CommitmentComparison {
        if (self.excess - other.excess).abs() > TOLERANCE {
            if self.excess < other.excess {
                CommitmentComparison::LowerExcess
            } else {
                CommitmentComparison::HigherExcess
            }
        } else if self.cost < other.cost - TOLERANCE {
            CommitmentComparison::LowerCost
        } else if other.cost < self.cost - TOLERANCE {
            CommitmentComparison::HigherCost
        } else {
            CommitmentComparison::EqualScore
        }
    }
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

/// The same bounded search as the synchronous query, with cooperative step boundaries.
pub struct AutomaticCommitmentSearch {
    session: GameSession,
    order: VecDeque<PlantId>,
    plan: CommitmentPlan,
    best: Evaluation,
    floor: Floor,
    reference_score: CommitmentScore,
    decisions: Vec<CommitmentDecision>,
    current: Option<PlantSearch>,
}

struct PlantSearch {
    plant: PlantId,
    kind: PlantKind,
    grid: NodeId,
    before: CommitmentScore,
    valid_candidates: usize,
    capacity_screened: usize,
    evaluation_limited: usize,
    all_off_screen_margin_mw: f64,
    remaining: VecDeque<[bool; 24]>,
    evaluated: Vec<CommitmentCandidate>,
    chosen: Option<(CommitmentSchedule, Evaluation)>,
}

impl AutomaticCommitmentSearch {
    /// Takes an owned snapshot; no live simulation state is touched.
    pub fn new(session: GameSession) -> Self {
        let mut plants: Vec<&Plant> = session
            .trimmable_ids()
            .into_iter()
            .filter_map(|id| session.power.plant(id))
            .collect();
        plants.sort_by(|a, b| {
            b.spec()
                .marginal_cost
                .value()
                .total_cmp(&a.spec().marginal_cost.value())
                .then(b.id().cmp(&a.id()))
        });
        let order = plants.into_iter().map(Plant::id).collect();
        let plan = session.all_day_plan();
        let best = session.evaluate(&plan);
        let floor = best.floor();
        let reference_score = best.score(&floor);
        Self {
            session,
            order,
            plan,
            best,
            floor,
            reference_score,
            decisions: Vec::new(),
            current: None,
        }
    }

    fn begin_plant(&self, id: PlantId) -> PlantSearch {
        let plant = self
            .session
            .power
            .plant(id)
            .expect("search owns its source fleet");
        let spec = plant.commitment_spec();
        let grid = self.session.grid_of(plant.node());
        let slack = self.best.hourly_slack(&self.floor);
        let slack = slack.get(&grid).copied().unwrap_or([f64::INFINITY; 24]);
        let credit = plant.available_capacity().value();
        let saving_per_hour = plant.min_output().value() * spec.marginal_cost.value();
        let startup = spec.startup_cost.cents() as f64 / 100.0;
        let valid = candidates(|hours| CommitmentSchedule::try_new(hours, &spec).is_ok());
        let valid_candidates = valid.len();
        let mut screened: Vec<(f64, [bool; 24])> = valid
            .into_iter()
            .filter(|hours| (0..24).all(|h| hours[h] || slack[h] >= credit - TOLERANCE))
            .map(|hours| {
                let off = hours.iter().filter(|on| !**on).count() as f64;
                let estimate = off * saving_per_hour - f64::from(restarts(hours)) * startup;
                (estimate, hours)
            })
            .collect();
        screened.sort_by(|a, b| b.0.total_cmp(&a.0));
        PlantSearch {
            plant: id,
            kind: plant.kind(),
            grid,
            before: self.best.score(&self.floor),
            valid_candidates,
            capacity_screened: valid_candidates - screened.len(),
            evaluation_limited: screened.len().saturating_sub(FULL_EVALUATIONS),
            all_off_screen_margin_mw: slack.into_iter().fold(f64::INFINITY, f64::min) - credit,
            remaining: screened
                .into_iter()
                .take(FULL_EVALUATIONS)
                .map(|(_, hours)| hours)
                .collect(),
            evaluated: Vec::new(),
            chosen: None,
        }
    }

    /// Returns true when complete. Repeated calls after completion are harmless.
    pub fn advance(&mut self) -> bool {
        if self.current.is_none() {
            let Some(id) = self.order.pop_front() else {
                return true;
            };
            self.current = Some(self.begin_plant(id));
        }
        let current = self
            .current
            .as_mut()
            .expect("plant search just initialized");
        if let Some(hours) = current.remaining.pop_front() {
            let plant = self
                .session
                .power
                .plant(current.plant)
                .expect("owned source fleet");
            let Ok(schedule) = CommitmentSchedule::try_new(hours, &plant.commitment_spec()) else {
                return false;
            };
            let mut trial = self.plan.clone();
            trial.insert(current.plant, schedule);
            let evaluation = self.session.evaluate(&trial);
            let incumbent = current.chosen.as_ref().map_or(&self.best, |(_, eval)| eval);
            let beats = evaluation.better_than(incumbent);
            let covers_floor = evaluation.covers(&self.floor);
            current.evaluated.push(CommitmentCandidate {
                hours,
                score: evaluation.score(&self.floor),
                compared_to: incumbent.score(&self.floor),
                covers_floor,
                comparison: evaluation.comparison(incumbent),
            });
            if covers_floor && beats {
                current.chosen = Some((schedule, evaluation));
            }
            return false;
        }
        let current = self.current.take().expect("completed plant search");
        if let Some((schedule, evaluation)) = current.chosen {
            self.plan.insert(current.plant, schedule);
            self.best = evaluation;
        }
        self.decisions.push(CommitmentDecision {
            plant: current.plant,
            kind: current.kind,
            grid: current.grid,
            before: current.before,
            selected: self.plan[&current.plant],
            selected_score: self.best.score(&self.floor),
            valid_candidates: current.valid_candidates,
            capacity_screened: current.capacity_screened,
            evaluation_limited: current.evaluation_limited,
            all_off_screen_margin_mw: current.all_off_screen_margin_mw,
            candidates: current.evaluated,
        });
        self.order.is_empty()
    }

    /// Completes any remaining steps, then returns the unchanged planner's final report.
    pub fn finish(mut self) -> CommitmentReport {
        while !self.advance() {}
        CommitmentReport {
            plan: self.plan,
            demand_cushion_factor: 1.0
                + 2.0 * self.session.region.demand_noise().forecast_error.value(),
            score_tolerance: TOLERANCE,
            evaluation_limit_per_plant: FULL_EVALUATIONS,
            reference_score: self.reference_score,
            selected_score: self.best.score(&self.floor),
            decisions: self.decisions,
        }
    }
}

impl GameSession {
    /// The commitment patterns that cover the coming day's forecast plus its cushion while
    /// burning the least expensive fuel (spec 2026-10-31 §4). Only connected, commissioned
    /// coal, CCGT and peaker plants on AGC are returned; everything else is left as it is.
    /// Pure: the session is unchanged.
    pub fn automatic_commitment_plan(&self) -> CommitmentPlan {
        self.automatic_commitment_report().plan
    }

    /// The same search, with the evidence behind every evaluated alternative. This does not
    /// evaluate extra candidates or change the tie-breaking policy.
    pub fn automatic_commitment_report(&self) -> CommitmentReport {
        self.automatic_commitment_search().finish()
    }

    /// An owned pure query that callers can advance between other work. Each step evaluates
    /// at most one candidate; stopping early never changes the source session.
    pub fn automatic_commitment_search(&self) -> AutomaticCommitmentSearch {
        AutomaticCommitmentSearch::new(self.clone())
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

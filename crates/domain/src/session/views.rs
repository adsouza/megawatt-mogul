//! Read-only views of the session for projections and display.

use std::collections::BTreeMap;

use super::{DayPhase, GameSession, Incident};
use crate::company::CapitalChange;
use crate::demand::{ForecastPoint, hourly_forecast};
use crate::generation::{Plant, PlantStatus};
use crate::grid::power_system::{HourEnd, day_hydro_budget};
use crate::grid::{GridMode, HydroScheduleView, LineStatus, TickSummary};
use crate::ids::{CellCoord, CityId, LineId, NodeId, PlantId};
use crate::region::PlantKind;
use crate::regulation::{Licence, compute_rate_review};
use crate::time::SimMonth;
use crate::units::{Hertz, MegawattHours, Megawatts, Money, Price, Ratio};

/// One plant as the player sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlantView {
    pub id: PlantId,
    pub kind: PlantKind,
    pub cell: CellCoord,
    pub node: NodeId,
    pub status: PlantStatus,
    pub output: Megawatts,
    pub auto_follow: bool,
    /// Whether the plant's node is energised from the substation.
    pub connected: bool,
    pub commitment: [bool; 24],
    pub hydro_budget: Option<MegawattHours>,
    /// The water a commissioned hydro plant starts the day of the current
    /// month with (spec §2.2): what Planning shows, since until the day
    /// begins `hydro_budget` is still the empty or leftover level. `None` for
    /// every other plant.
    pub hydro_day_budget: Option<MegawattHours>,
    /// The output the plant starts the day at when it is committed in hour 0:
    /// what Planning's `SetDispatchTarget` sets, minimum stable output until
    /// then.
    pub initial_target: Megawatts,
}

/// One city as the player sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct CityView {
    pub id: CityId,
    pub name_key: String,
    pub cell: CellCoord,
    pub node: NodeId,
    /// Whether the city is energised and therefore a customer.
    pub connected: bool,
}

/// One city's demand, for the coming day in Planning or the day being played
/// (spec §16.8). Every city shares one demand shape and one noise process,
/// and a rotating outage or protection's shed falls on connected cities in
/// proportion, so a connected city's figures are the system's scaled by its
/// share of the connected base load.
#[derive(Debug, Clone, PartialEq)]
pub struct CityDetail {
    pub id: CityId,
    pub name_key: String,
    /// Whether the city is energised and therefore a customer.
    pub connected: bool,
    pub base_load: Megawatts,
    /// `base_load` over the connected cities' total base load; zero for an
    /// unconnected city.
    pub share: Ratio,
    /// The city's forecast demand in the hour of the day's forecast peak, the
    /// mean at minute 30 as in [`GameSession::hourly_forecast`]. For an
    /// unconnected city, the demand it would add once connected.
    pub peak: Megawatts,
    /// The hour of [`Self::peak`], 0 to 23, the earliest on a tie.
    pub peak_hour: u8,
    /// The city's share of the last tick's demand, before any outage or
    /// shed: `Some` only while Operating, for a connected city, once a tick
    /// has run.
    pub demand_now: Option<Megawatts>,
    /// The city's share of the load the last tick served, when
    /// [`Self::demand_now`] is `Some`.
    pub served_now: Option<Megawatts>,
}

/// One transmission line as the player sees it. Direction carries no meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineView {
    pub id: LineId,
    pub from: NodeId,
    pub to: NodeId,
    pub status: LineStatus,
}

/// The grid's state at the current instant.
#[derive(Debug, Clone, PartialEq)]
pub struct GridSnapshot {
    pub month: SimMonth,
    pub tick: u32,
    pub phase: DayPhase,
    /// Absolute frequency: nominal plus deviation.
    pub frequency: Hertz,
    pub mode: GridMode,
    /// Protection's shed: a share of the load the rotating outage leaves, not
    /// of demand (spec §15.3), so the share of demand served is
    /// `(1 − outage) × (1 − shed)`.
    pub shed: Ratio,
    /// The rotating-outage share of demand cut on purpose now. Set at
    /// `BeginOperating` to hour 0's target, so it is meaningful before the
    /// first tick; zero during a blackout.
    pub outage: Ratio,
    /// The rotating-outage share ordered for the current hour, which
    /// `outage` moves toward; frozen during a blackout.
    pub outage_target: Ratio,
    /// The most recent tick of the current day, if any has run.
    pub last_tick: Option<TickSummary>,
}

/// The company's standing with its customers and regulator.
#[derive(Debug, Clone, PartialEq)]
pub struct CompanyView {
    pub cash: Money,
    pub reliability: Ratio,
    pub satisfaction: f64,
    pub tariff: Price,
    pub allowed_return: Ratio,
    pub strikes: u8,
    pub licence: Licence,
}

/// What the next annual reviews would do if they ran on today's state.
///
/// An estimate, not a promise: it uses the year so far and today's grid.
#[derive(Debug, Clone, PartialEq)]
pub struct RateReviewEstimate {
    /// The allowed return the rate review would set.
    pub projected_return: Ratio,
    /// Firm capacity over the review's peak, minus one: spare capacity as a
    /// fraction of that peak (zero when the peak is zero).
    pub reserve_margin: f64,
    /// Book value a first reserve review would hold out of the rate base.
    pub capital_to_hold: Money,
    /// Book value a reserve review would release back into the rate base.
    pub capital_to_release: Money,
    /// Held book value a second consecutive reserve review would write off
    /// permanently.
    pub capital_at_risk: Money,
}

/// The hour in which committed minimum generation exceeds the net load by
/// the most (spec §14.3): the plan would have to dump or curtail the surplus.
/// Its figures are those of the hour's worse end, minute 0 or minute 59.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimumGenerationWarning {
    /// The hour of day, 0 to 23.
    pub hour: u8,
    /// What the plants committed in that hour must generate at least.
    pub committed_minimum: Megawatts,
    /// The forecast net load at the hour's worse end: the lower of minute 0
    /// and minute 59.
    pub net_load: Megawatts,
    /// `committed_minimum − net_load`, always positive.
    pub excess: Megawatts,
}

/// The hour in which the committed fleet's deliverable capacity falls short
/// of the forecast demand by the most (spec §15.5): the plan would have to
/// leave that load to a rotating outage. Its figures are those of the hour's
/// worse end, minute 0 or minute 59.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapacityShortfallWarning {
    /// The hour of day, 0 to 23.
    pub hour: u8,
    /// The connected forecast demand at the hour's worse end.
    pub demand: Megawatts,
    /// What the plants committed in that hour could deliver then, with
    /// expected solar: an upper bound that ignores start-up and ramp limits.
    pub deliverable: Megawatts,
    /// `demand − deliverable`, always positive.
    pub shortfall: Megawatts,
}

impl GameSession {
    /// Every plant, in id order.
    pub fn plant_views(&self) -> Vec<PlantView> {
        self.power
            .plants()
            .values()
            .map(|plant| self.plant_view(plant))
            .collect()
    }

    fn plant_view(&self, plant: &Plant) -> PlantView {
        PlantView {
            id: plant.id(),
            kind: plant.kind(),
            cell: plant.cell(),
            node: plant.node(),
            status: plant.status(),
            output: plant.output(),
            auto_follow: plant.auto_follow(),
            connected: self.power.network().is_connected(plant.node()),
            commitment: plant.commitment().hours(),
            hydro_budget: plant.hydro_budget(),
            hydro_day_budget: (plant.kind() == PlantKind::Hydro
                && !matches!(plant.status(), PlantStatus::UnderConstruction { .. }))
            .then(|| day_hydro_budget(&self.region, self.clock.month(), plant)),
            initial_target: plant.initial_target(),
        }
    }

    /// Every city, in id order.
    pub fn city_views(&self) -> Vec<CityView> {
        self.power
            .cities()
            .values()
            .map(|city| CityView {
                id: city.id(),
                name_key: city.def().name_key.clone(),
                cell: city.def().cell,
                node: city.node(),
                connected: self.power.network().is_connected(city.node()),
            })
            .collect()
    }

    /// One city's demand figures (spec §16.8), or `None` for an id the
    /// session doesn't know. Pure: no randomness, no state change.
    pub fn city_view(&self, id: CityId) -> Option<CityDetail> {
        let city = self.power.cities().get(&id)?;
        let network = self.power.network();
        let connected = network.is_connected(city.node());
        let base_load = city.def().base_load;
        let connected_load: Megawatts = self
            .power
            .cities()
            .values()
            .filter(|other| network.is_connected(other.node()))
            .map(|other| other.def().base_load)
            .sum();
        let share = if connected && connected_load > Megawatts::ZERO {
            Ratio::new((base_load / connected_load).clamp(0.0, 1.0)).unwrap_or(Ratio::ZERO)
        } else {
            Ratio::ZERO
        };
        // The city's own forecast: the system's shape on its base load alone, so
        // an unconnected city shows what it would add.
        let forecast = hourly_forecast([city], self.region.demand(), self.clock.month());
        let peak = forecast.iter().fold(forecast[0], |best, point| {
            if point.mean > best.mean { *point } else { best }
        });
        let live = (connected && self.phase == DayPhase::Operating)
            .then(|| self.day_ticks.last())
            .flatten();
        Some(CityDetail {
            id,
            name_key: city.def().name_key.clone(),
            connected,
            base_load,
            share,
            peak: peak.mean,
            peak_hour: peak.hour,
            demand_now: live.map(|tick| share.of(tick.demand)),
            served_now: live.map(|tick| share.of(tick.served)),
        })
    }

    /// Every transmission line, in id order, built or under construction.
    pub fn line_views(&self) -> Vec<LineView> {
        self.power
            .network()
            .lines()
            .map(|line| LineView {
                id: line.id,
                from: line.from,
                to: line.to,
                status: line.status,
            })
            .collect()
    }

    /// The noise-free demand forecast of connected cities for each hour of
    /// the current month's representative day.
    pub fn hourly_forecast(&self) -> [ForecastPoint; 24] {
        let network = self.power.network();
        let connected = self
            .power
            .cities()
            .values()
            .filter(|city| network.is_connected(city.node()));
        hourly_forecast(connected, self.region.demand(), self.clock.month())
    }

    /// Today's hydro peak-shaving schedule (spec §12.6), valid per phase:
    /// the current plan while Operating, the finished day with every hour
    /// frozen during Review, and empty during Planning (see
    /// [`crate::grid::PowerSystem::hydro_schedule`]).
    pub fn hydro_schedule(&self) -> HydroScheduleView {
        self.power.hydro_schedule()
    }

    /// The played day's incidents so far, oldest first, each with its tick
    /// (spec §8.4: the Operating HUD's incident ticker). The day starts empty
    /// at `BeginOperating`, and the month close publishes them in the report
    /// and starts empty again.
    pub fn incidents_today(&self) -> &[Incident] {
        self.month_to_date.incidents()
    }

    /// The energy each plant has produced so far in the played day, unweighted (the month close
    /// weights it by the month's days). Only plants that were connected and commissioned at some
    /// tick are listed. The day starts empty at `BeginOperating`, and the month close reports it
    /// and starts empty again. A physical figure only: nothing is priced before the close.
    pub fn plant_energy_today(&self) -> &BTreeMap<PlantId, MegawattHours> {
        self.month_to_date.plant_energy()
    }

    pub fn grid_snapshot(&self) -> GridSnapshot {
        let protection = self.power.protection();
        GridSnapshot {
            month: self.clock.month(),
            tick: self.clock.tick(),
            phase: self.phase,
            frequency: self.region.frequency().nominal + self.power.deviation(),
            mode: protection.mode,
            shed: protection.shed,
            outage: self.power.outage(),
            outage_target: self.power.outage_target(),
            last_tick: self.day_ticks.last().cloned(),
        }
    }

    pub fn company_view(&self) -> CompanyView {
        CompanyView {
            cash: self.company.cash(),
            reliability: self.company.reliability(),
            satisfaction: self.company.satisfaction(),
            tariff: self.regulator.tariff(),
            allowed_return: self.regulator.allowed_return(),
            strikes: self.regulator.strikes(),
            licence: self.regulator.licence(),
        }
    }

    /// Expected (noise-free) demand of connected cities less the expected
    /// output of connected solar at `tick` of the current month: the load the
    /// dispatchable fleet has to follow.
    pub fn forecast_net_load(&self, tick: u32) -> Megawatts {
        let month = self.clock.month();
        self.power
            .connected_demand_forecast(&self.region, month, tick)
            - self.power.expected_solar(&self.region, month, tick)
    }

    /// The Planning-time check of the commitment plan against the load
    /// forecast (spec §14.3): the hour whose committed minimum generation
    /// exceeds its forecast net load by the most, earliest on ties; `None`
    /// when every hour's minimums fit under its net load.
    ///
    /// Each hour is judged at both ends, minute 0 and minute 59, where its
    /// interpolated load has its extremes, by the evaluator the capacity
    /// check and operating-time sizing use ([`HourEnd`]); the hour's excess
    /// is that of its worse end, the one with the lower net load, minute 0 on
    /// a tie, and the warning's `net_load` is that end's. Reads the plants'
    /// commitment patterns (not the per-day suppression override), so it is
    /// meaningful while planning. Pure: no randomness, no state change.
    pub fn minimum_generation_check(&self) -> Option<MinimumGenerationWarning> {
        let hours = self
            .power
            .committed_minimum_hour_ends(&self.region, self.clock.month());
        let mut worst: Option<MinimumGenerationWarning> = None;
        for (hour, (committed_minimum, ends)) in hours.into_iter().enumerate() {
            let end = HourEnd::worse_by(ends, |end| committed_minimum - end.net_load());
            let net_load = end.net_load();
            let excess = committed_minimum - net_load;
            let beats_worst = worst.map_or(excess > Megawatts::ZERO, |w| excess > w.excess);
            if beats_worst {
                worst = Some(MinimumGenerationWarning {
                    hour: hour as u8,
                    committed_minimum,
                    net_load,
                    excess,
                });
            }
        }
        worst
    }

    /// The Planning-time check of the commitment plan against the demand
    /// forecast (spec §15.5): the hour whose forecast demand exceeds what the
    /// committed fleet could deliver by the most, earliest on ties; `None`
    /// when every hour is covered.
    ///
    /// Each hour is judged at both ends, minute 0 and minute 59, where its
    /// interpolated demand has its extremes, by the evaluator operating-time
    /// sizing uses (spec §15.3); the hour's shortfall is that of its worse
    /// end, minute 0 on a tie. An upper bound: it reads the plants'
    /// commitment patterns (not the per-day suppression override), ignores
    /// start-up and ramp limits, and counts scheduled hydro from a pure
    /// preview of the coming day's plan, since the live plan is empty during
    /// Planning. Hydro is the one, safe, exception to the bound: each
    /// boundary re-plan pours water left unused earlier in the day, so the
    /// live allocation can exceed the preview. Pure: no randomness, no state
    /// change.
    pub fn capacity_shortfall_check(&self) -> Option<CapacityShortfallWarning> {
        let ends = self
            .power
            .planned_hour_ends(&self.region, self.clock.month());
        let mut worst: Option<CapacityShortfallWarning> = None;
        for (hour, end) in ends.into_iter().map(HourEnd::worse).enumerate() {
            let shortfall = end.shortfall();
            let beats_worst =
                worst.map_or(shortfall > Megawatts::ZERO, |w| shortfall > w.shortfall);
            if beats_worst {
                worst = Some(CapacityShortfallWarning {
                    hour: hour as u8,
                    demand: end.demand,
                    deliverable: end.deliverable,
                    shortfall,
                });
            }
        }
        worst
    }

    /// What the next reserve review and rate review would do on today's
    /// state. Pure: the reserve review is only previewed, so held portions
    /// and their review counts are untouched.
    ///
    /// The review peak is `max(year's peak so far, projected peak)`, as in
    /// December's real review, with the projection starting next month.
    pub fn rate_review_estimate(&self) -> RateReviewEstimate {
        let inputs = self.review_inputs(self.clock.month());
        let capital = self
            .company
            .rate_base()
            .preview_reserve_review(&inputs.reserve);
        let (firm, review_peak) = (inputs.firm_capacity(), inputs.review_peak());
        let outcome = compute_rate_review(
            &inputs.rate_review(capital.eligible_total),
            self.region.regulator(),
        );

        let (mut hold, mut release, mut at_risk) = (Money::ZERO, Money::ZERO, Money::ZERO);
        for change in &capital.changes {
            match change {
                CapitalChange::Held { amount, .. } => hold += *amount,
                CapitalChange::Released { amount, .. } => release += *amount,
                CapitalChange::Disallowed { amount, .. } => at_risk += *amount,
            }
        }
        RateReviewEstimate {
            projected_return: outcome.allowed_return,
            reserve_margin: if review_peak > Megawatts::ZERO {
                firm / review_peak - 1.0
            } else {
                0.0
            },
            capital_to_hold: hold,
            capital_to_release: release,
            capital_at_risk: at_risk,
        }
    }
}

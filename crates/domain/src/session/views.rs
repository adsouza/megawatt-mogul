//! Read-only views of the session for projections and display.

use std::collections::BTreeMap;

use super::{DayPhase, GameSession, Incident};
use crate::company::CapitalChange;
use crate::demand::{ForecastPoint, hourly_forecast};
use crate::generation::{BatteryFlow, BatteryMode, BatteryTotals, Plant, PlantStatus};
use crate::grid::power_system::day_hydro_budget;
use crate::grid::{GridMode, HydroScheduleView, LineStatus, TickSummary};
use crate::ids::{CellCoord, CityId, LineId, NodeId, PlantId};
use crate::region::PlantKind;
use crate::regulation::{Licence, compute_rate_review};
use crate::time::{SimMonth, TICKS_PER_HOUR};
use crate::units::{Hertz, MegawattHours, Megawatts, Money, Price, Ratio};

/// A sample used by the minimum-generation warning, before optional battery charging.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimumGenerationPoint {
    pub tick: u32,
    pub committed_minimum: Megawatts,
    pub net_load: Megawatts,
}

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
    /// Whether the plant's node is energised from the home city.
    pub connected: bool,
    pub commitment: [bool; 24],
    pub hydro_budget: Option<MegawattHours>,
    pub battery: Option<BatteryView>,
    pub wind: Option<WindView>,
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

/// Resource and forecast of one wind farm, with no assumed statistical confidence level.
#[derive(Debug, Clone, PartialEq)]
pub struct WindView {
    pub potential: Ratio,
    pub available: Megawatts,
    pub delivered: Megawatts,
    pub curtailed: Megawatts,
    pub forecast: (Megawatts, Megawatts, Megawatts),
}

/// All storage values are physical daily quantities; costs are in the plant specification.
#[derive(Debug, Clone, PartialEq)]
pub struct BatteryView {
    pub enabled: bool,
    pub mode: BatteryMode,
    pub stored: MegawattHours,
    pub spec: crate::region::BatterySpec,
    pub flow: BatteryFlow,
    pub today: BatteryTotals,
    pub charge_window: (f64, f64),
}

/// One city as the player sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct CityView {
    pub id: CityId,
    pub name_key: String,
    pub cell: CellCoord,
    pub node: NodeId,
    /// Whether the city is an active customer; unsupported remote islands wait inactive.
    pub connected: bool,
}

/// One city's demand, for the coming day in Planning or the day being played
/// (spec §16.8). Cities use their local climate and share one noise process.
/// Outages and shedding fall in proportion to their instantaneous forecast
/// demand. `share` continues to describe their share of connected base load.
#[derive(Debug, Clone, PartialEq)]
pub struct CityDetail {
    pub id: CityId,
    pub name_key: String,
    /// Whether the city is an active customer; a physical route alone is insufficient.
    pub connected: bool,
    /// A physical route exists, but this remote island has no commissioned synchronous source.
    pub inactive: bool,
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
/// Its figures use the worst sampled minute: 0, 59, or an interior local-hour bend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimumGenerationWarning {
    /// The hour of day, 0 to 23.
    pub hour: u8,
    /// What the plants committed in that hour must generate at least.
    pub committed_minimum: Megawatts,
    /// The lowest forecast net load among the hour's endpoints and interior
    /// local-hour bends.
    pub net_load: Megawatts,
    /// `committed_minimum − net_load`, always positive.
    pub excess: Megawatts,
}

/// The hour in which the committed fleet's deliverable capacity falls short
/// of the forecast demand by the most (spec §15.5): the plan would have to
/// leave that load to a rotating outage. Its figures are those of the hour's
/// worst sampled minute: 0, 59, or an interior local-hour bend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapacityShortfallWarning {
    /// The hour of day, 0 to 23.
    pub hour: u8,
    /// The connected forecast demand at that sampled minute.
    pub demand: Megawatts,
    /// What the plants committed in that hour could deliver then, with
    /// expected solar: an upper bound that ignores start-up and ramp limits.
    pub deliverable: Megawatts,
    /// `demand − deliverable`, always positive.
    pub shortfall: Megawatts,
}

impl GameSession {
    fn battery_view(&self, p: &Plant) -> Option<BatteryView> {
        let (Some(spec), Some(b)) = (p.spec().battery, p.battery()) else {
            return None;
        };
        let connected = self.power.network().is_connected(p.node());
        let f = b.flow();
        let mode = if !b.enabled() {
            BatteryMode::Disabled
        } else if !connected {
            BatteryMode::Disconnected
        } else if self.power.mode_at(p.node()) == GridMode::Blackout {
            BatteryMode::GridSuspended
        } else if f.charge > Megawatts::ZERO {
            BatteryMode::Charging
        } else if f.discharge > Megawatts::ZERO {
            BatteryMode::Discharging
        } else if b.stored() == MegawattHours::ZERO {
            BatteryMode::Empty
        } else if spec.energy_capacity.value() - b.stored().value() <= 1e-9 {
            BatteryMode::Full
        } else {
            BatteryMode::Holding
        };
        Some(BatteryView {
            enabled: b.enabled(),
            mode,
            stored: b.stored(),
            spec,
            flow: f,
            today: self
                .month_to_date
                .storage()
                .get(&p.id())
                .copied()
                .unwrap_or_default(),
            charge_window: (0.0, f64::from(spec.charge_end_hour)),
        })
    }
    pub fn battery_schedule(&self) -> crate::grid::BatteryScheduleView {
        if self.region.areas().is_some() && !self.power.island_nodes().is_empty() {
            let mut total = crate::grid::BatteryScheduleView::default();
            for id in self.power.island_nodes().keys() {
                if let Some(scoped) = self.island_session(*id) {
                    let next = scoped.battery_schedule();
                    total.plants.extend(next.plants);
                    total.expected_charging |= next.expected_charging;
                }
            }
            return total;
        }
        if self.phase == DayPhase::Planning {
            self.power
                .preview_batteries(&self.region, self.clock.month())
        } else {
            self.power.battery_schedule()
        }
    }

    /// The coming day's minute-by-minute supply with feasible battery flows,
    /// from the Planning shortfall evaluator. Starts storage empty and credits
    /// only charging available from forecast surplus. Pure; leaves live water,
    /// storage, schedules and RNG untouched. Empty without enabled, connected,
    /// commissioned storage. Unlike a battery schedule, these are feasible
    /// flows after customer demand, generation and hydro water are accounted for.
    pub fn battery_supply_forecast(&self) -> Vec<crate::grid::SupplyForecastPoint> {
        self.power
            .battery_supply_forecast(&self.region, self.clock.month())
    }

    /// The coming day's expected supply, using the shortfall check's rules for
    /// connectivity, commissioning, commitments, manual targets, hydro and storage.
    /// With storage or wind, every minute is assessed; otherwise the warning
    /// assesses hour endpoints and connected cities' local-hour bends.
    /// Pure: no live water, energy or RNG changes.
    pub fn supply_forecast(&self) -> crate::grid::SupplyForecast {
        if self.region.areas().is_some() && !self.power.island_nodes().is_empty() {
            let mut forecasts = self
                .power
                .island_nodes()
                .keys()
                .filter_map(|id| self.power.scoped_island(*id, &self.region))
                .map(|p| p.supply_forecast(&self.region, self.clock.month()));
            if let Some(mut total) = forecasts.next() {
                for next in forecasts {
                    total.has_storage |= next.has_storage;
                    total.assess_every_minute |= next.assess_every_minute;
                    total.hour_sample_minutes.extend(next.hour_sample_minutes);
                    for (a, b) in total.minutes.iter_mut().zip(next.minutes) {
                        a.demand += b.demand;
                        a.gross_supply += b.gross_supply;
                        a.battery_charge += b.battery_charge;
                        a.battery_discharge += b.battery_discharge;
                        for (k, v) in b.generation {
                            *a.generation.entry(k).or_insert(Megawatts::ZERO) += v;
                        }
                    }
                }
                return total;
            }
        }
        self.power.supply_forecast(&self.region, self.clock.month())
    }

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
            battery: self.battery_view(plant),
            wind: (plant.kind() == PlantKind::Wind).then(|| {
                let potential = self
                    .region
                    .terrain()
                    .get(plant.cell())
                    .map_or(Ratio::ZERO, |c| c.wind_potential);
                let rating = plant.spec().capacity * potential.value();
                let (actual, (central, low, high)) = self.power.wind_for_node(
                    plant.node(),
                    &self.region,
                    self.clock.month(),
                    self.clock.tick(),
                );
                let available = rating * actual;
                let online = plant.is_online() && self.power.network().is_connected(plant.node());
                WindView {
                    potential,
                    available,
                    delivered: if online {
                        plant.output()
                    } else {
                        Megawatts::ZERO
                    },
                    curtailed: if online {
                        (available - plant.output()).max(Megawatts::ZERO)
                    } else {
                        Megawatts::ZERO
                    },
                    forecast: (rating * central, rating * low, rating * high),
                }
            }),
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
                connected: self.power.node_active(city.node()),
            })
            .collect()
    }

    /// One city's demand figures (spec §16.8), or `None` for an id the
    /// session doesn't know. Pure: no randomness, no state change.
    pub fn city_detail(&self, id: CityId) -> Option<CityDetail> {
        let city = self.power.cities().get(&id)?;
        let network = self.power.network();
        let connected = self.power.node_active(city.node());
        let inactive = network.is_connected(city.node()) && !connected;
        let base_load = city.def().base_load;
        let connected_load: Megawatts = self
            .power
            .cities()
            .values()
            .filter(|other| self.power.node_active(other.node()))
            .map(|other| other.def().base_load)
            .sum();
        let share = if connected && connected_load > Megawatts::ZERO {
            Ratio::new((base_load / connected_load).clamp(0.0, 1.0)).unwrap_or(Ratio::ZERO)
        } else {
            Ratio::ZERO
        };
        // The city's own climate in its local time, including the load an
        // unconnected city would add.
        let forecast = hourly_forecast([city], &self.region, self.clock.month());
        let peak = forecast.iter().fold(forecast[0], |best, point| {
            if point.mean > best.mean { *point } else { best }
        });
        let live = (connected && self.phase == DayPhase::Operating)
            .then(|| self.day_ticks.last())
            .flatten();
        let live_share = live.map(|tick| {
            let month = self.clock.month();
            let factor = self.region.load_factor(id, month, tick.tick);
            let mut identical = true;
            let total: Megawatts = self
                .power
                .cities()
                .values()
                .filter(|other| self.power.node_active(other.node()))
                .map(|other| {
                    let other_factor = self.region.load_factor(other.id(), month, tick.tick);
                    identical &= other_factor == factor;
                    other.def().base_load * other_factor
                })
                .sum();
            // Preserve the original base-share arithmetic for uniform climates,
            // including every legacy region. Avoid cancellation roundoff.
            if identical {
                share
            } else if total > Megawatts::ZERO {
                Ratio::new(((base_load * factor) / total).clamp(0.0, 1.0)).unwrap_or(Ratio::ZERO)
            } else {
                Ratio::ZERO
            }
        });
        let served_now = live.zip(live_share).map(|(tick, weight)| {
            let row = self
                .power
                .island_nodes()
                .iter()
                .find(|(_, nodes)| nodes.contains(&city.node()))
                .and_then(|(island, _)| tick.islands.get(island));
            row.map_or_else(
                || weight.of(tick.served),
                |row| {
                    if row.demand <= Megawatts::ZERO {
                        return Megawatts::ZERO;
                    }
                    // Shared regional noise preserves city demand shares. Service cuts are local
                    // to the island, so a neighbouring island cannot cover this city's outage.
                    row.served * (weight.of(tick.demand) / row.demand).clamp(0.0, 1.0)
                },
            )
        });
        Some(CityDetail {
            id,
            name_key: city.def().name_key.clone(),
            connected,
            inactive,
            base_load,
            share,
            peak: peak.mean,
            peak_hour: peak.hour,
            demand_now: live
                .zip(live_share)
                .map(|(tick, weight)| weight.of(tick.demand)),
            served_now,
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

    /// Month closes until an ordered path links this city to the home grid.
    /// Zero for an existing connection, `None` for an unknown or unreachable city.
    pub fn city_connection_months(&self, city: CityId) -> Option<u32> {
        let city = self.power.cities().get(&city)?;
        self.power.network().months_until_connected(city.node())
    }

    /// The months `line` takes to build in all: what its quote said when it was ordered, by the
    /// rule [`GameSession::build_line`] charges, so a line under construction can show its total
    /// beside its months left (spec §16.9). It reads the line's stored length, as the month close
    /// does when it books the line. Pure. `None` for a line the network doesn't have.
    pub fn line_build_months(&self, line: LineId) -> Option<u32> {
        Some(self.line_construction(line)?.months)
    }

    /// The noise-free demand forecast of connected cities for each hour of
    /// the current month's representative day.
    pub fn hourly_forecast(&self) -> [ForecastPoint; 24] {
        let connected = self
            .power
            .cities()
            .values()
            .filter(|city| self.power.node_active(city.node()));
        hourly_forecast(connected, &self.region, self.clock.month())
    }

    /// Today's hydro peak-shaving schedule (spec §12.6), valid per phase:
    /// the current plan while Operating, the finished day with every hour
    /// frozen during Review, and empty during Planning (see
    /// [`crate::grid::PowerSystem::hydro_schedule`]).
    pub fn hydro_schedule(&self) -> HydroScheduleView {
        if self.region.areas().is_some() && !self.power.island_nodes().is_empty() {
            let mut total = HydroScheduleView::default();
            for id in self.power.island_nodes().keys() {
                if let Some(p) = self.power.scoped_island(*id, &self.region) {
                    total.plants.extend(p.hydro_schedule().plants);
                }
            }
            return total;
        }
        self.power.hydro_schedule()
    }

    /// The coming day's hourly allocations for connected, commissioned hydro
    /// on AGC, from the same water-aware preview as the Planning shortfall
    /// check. Battery reserves do not reduce its allocation. Manual hydro is absent:
    /// the Planning check counts its initial target instead. Pure; leaves the
    /// live schedule, water budgets and RNG untouched.
    pub fn hydro_forecast(&self) -> BTreeMap<PlantId, [Megawatts; 24]> {
        self.power
            .preview_hydro(&self.region, self.clock.month())
            .into_iter()
            .map(|(id, fill)| (id, fill.hourly))
            .collect()
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
            - self.power.expected_variable(&self.region, month, tick)
            - self.power.scheduled_injection(tick)
    }

    /// The generators' dispatch requirement, including planned charging.
    /// Battery discharge bridges actual gaps without reducing this requirement. Pure.
    pub fn forecast_dispatch_net_load(&self, tick: u32) -> Megawatts {
        self.forecast_net_load(tick)
            + self
                .power
                .storage_charging_requirement(self.clock.tick(), tick)
    }

    /// The Planning-time check of the commitment plan against the load
    /// forecast (spec §14.3): the hour whose committed minimum generation
    /// exceeds its forecast net load by the most, earliest on ties; `None`
    /// when every hour's minimums fit under its net load.
    ///
    /// Each hour is judged at its endpoints and connected cities' local-hour
    /// bends by the evaluator the capacity check and operating-time sizing use
    /// (`HourEnd`); the hour's excess is that of the sample with the lower
    /// net load, earliest on ties, and the warning's `net_load` is that sample's.
    /// Reads the plants'
    /// commitment patterns (not the per-day suppression override), so it is
    /// meaningful while planning. Pure: no randomness, no state change.
    pub fn minimum_generation_check(&self) -> Option<MinimumGenerationWarning> {
        if self.region.areas().is_some() && !self.power.island_nodes().is_empty() {
            return self
                .power
                .island_nodes()
                .keys()
                .filter_map(|id| self.island_session(*id)?.minimum_generation_check())
                .reduce(|first, next| {
                    if next.excess > first.excess {
                        next
                    } else {
                        first
                    }
                });
        }
        let mut worst: Option<MinimumGenerationWarning> = None;
        for MinimumGenerationPoint {
            tick,
            committed_minimum,
            net_load,
        } in self.minimum_generation_forecast()
        {
            let excess = committed_minimum - net_load;
            let beats_worst = worst.map_or(excess > Megawatts::ZERO, |w| excess > w.excess);
            if beats_worst {
                worst = Some(MinimumGenerationWarning {
                    hour: (tick / TICKS_PER_HOUR) as u8,
                    committed_minimum,
                    net_load,
                    excess,
                });
            }
        }
        worst
    }

    /// Exact minimum-warning samples for one controller. For multi-area regions,
    /// call on `island_session(id)` so other islands cannot absorb a local surplus.
    pub fn minimum_generation_forecast(&self) -> Vec<MinimumGenerationPoint> {
        let minutes = self.region.hour_sample_minutes(
            self.power
                .cities()
                .values()
                .filter(|city| self.power.node_active(city.node()))
                .map(|city| city.id()),
            self.clock.month(),
        );
        self.power
            .committed_minimum_hour_ends(&self.region, self.clock.month())
            .into_iter()
            .enumerate()
            .flat_map(|(hour, (committed_minimum, ends))| {
                minutes
                    .iter()
                    .copied()
                    .zip(ends)
                    .map(move |(minute, end)| MinimumGenerationPoint {
                        tick: hour as u32 * TICKS_PER_HOUR + minute,
                        committed_minimum,
                        net_load: end.net_load(),
                    })
            })
            .collect()
    }

    /// The Planning-time check of the commitment plan against the demand
    /// forecast (spec §15.5): the hour whose forecast demand exceeds what the
    /// committed fleet could deliver by the most, earliest on ties; `None`
    /// when every hour is covered.
    ///
    /// Each hour is judged at its endpoints and connected cities' local-hour
    /// bends, or every minute for fleets with storage or wind, matching
    /// operating-time sizing (spec §15.3). The worst sample determines the
    /// hour's shortfall, earliest on ties. An upper bound: it reads the plants'
    /// commitment patterns (not the per-day suppression override), ignores
    /// start-up and ramp limits, and counts scheduled hydro from a pure
    /// preview of the coming day's plan, since the live plan is empty during
    /// Planning. Hydro is the one, safe, exception to the bound: each
    /// boundary re-plan pours water left unused earlier in the day, so the
    /// live allocation can exceed the preview. Pure: no randomness, no state
    /// change.
    pub fn capacity_shortfall_check(&self) -> Option<CapacityShortfallWarning> {
        if self.region.areas().is_some() && !self.power.island_nodes().is_empty() {
            return self
                .power
                .island_nodes()
                .keys()
                .filter_map(|id| self.island_session(*id)?.capacity_shortfall_check())
                .reduce(|first, next| {
                    if next.shortfall > first.shortfall {
                        next
                    } else {
                        first
                    }
                });
        }
        Self::capacity_shortfall_for(&self.supply_forecast())
    }

    /// Assesses an already-computed supply forecast, so callers can share it with charts.
    /// Uses the same assessed minutes and earliest-on-ties rule as `capacity_shortfall_check`.
    pub fn capacity_shortfall_for(
        forecast: &crate::grid::SupplyForecast,
    ) -> Option<CapacityShortfallWarning> {
        let mut worst: Option<CapacityShortfallWarning> = None;
        for (tick, point) in forecast.assessed_minutes() {
            let shortfall = point.demand - point.gross_supply;
            let beats_worst =
                worst.map_or(shortfall > Megawatts::ZERO, |w| shortfall > w.shortfall);
            if beats_worst {
                worst = Some(CapacityShortfallWarning {
                    hour: (tick / TICKS_PER_HOUR) as u8,
                    demand: point.demand,
                    deliverable: point.gross_supply,
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
        let mut rate_base = self.company.rate_base().clone();
        rate_base.refresh_plant_firm(|id| {
            self.power
                .plant(id)
                .filter(|p| !matches!(p.status(), PlantStatus::UnderConstruction { .. }))
                .map(|p| {
                    p.spec()
                        .firm_capacity(&self.region.regulator().storage_accreditation)
                })
        });
        let link_changes = rate_base.review_link_utilisation(
            self.clock.month().year(),
            &self.link_utilisation(self.clock.month()),
        );
        let capital = rate_base.preview_reserve_review(&inputs.reserve);
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
        for event in link_changes {
            match event {
                crate::events::DomainEvent::CapitalHeld { amount, .. } => hold += amount,
                crate::events::DomainEvent::CapitalReleased { amount, .. } => release += amount,
                crate::events::DomainEvent::CapitalDisallowed { amount, .. } => at_risk += amount,
                _ => {}
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

    /// Current satisfaction's contribution to the next allowed return; never persisted.
    pub fn satisfaction_return_adjustment(&self) -> f64 {
        crate::regulation::satisfaction_adjustment(
            self.company.satisfaction(),
            self.region.regulator(),
        )
    }
}

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(RateReviewEstimate {
    projected_return,
    reserve_margin,
    capital_to_hold,
    capital_to_release,
    capital_at_risk
});

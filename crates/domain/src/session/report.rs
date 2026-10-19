//! What a played month did: the running accumulator filled tick by tick, and
//! the report the month close publishes from it.
//!
//! The accumulator holds the representative day's physical quantities
//! unweighted. The report weights every energy figure by the month's days,
//! so a 30-day month reports thirty copies of its one played day.

use std::collections::{BTreeMap, BTreeSet};

use crate::events::DomainEvent;
use crate::generation::{Plant, PlantStatus};
use crate::grid::{GridMode, TickSummary};
use crate::ids::PlantId;
use crate::region::PlantKind;
use crate::regulation::RateReviewOutcome;
use crate::time::SimMonth;
use crate::units::{Hertz, MegawattHours, Megawatts, Money, Price, Ratio};

/// A protection action worth reporting to the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IncidentKind {
    /// Under-frequency protection shed more load; `mw` is the total now shed.
    LoadShed {
        mw: Megawatts,
    },
    Blackout,
    BlackoutRestored,
    /// Over-frequency protection tripped `plant`.
    OverFrequencyTrip {
        plant: PlantId,
    },
}

/// An incident and the tick of the played day it happened in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Incident {
    pub tick: u32,
    pub kind: IncidentKind,
}

/// One connected plant's place in the month's dispatch stack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeritOrderEntry {
    pub plant: PlantId,
    pub kind: PlantKind,
    pub marginal_cost: Price,
    /// Energy over the month (weighted by days).
    pub energy: MegawattHours,
}

/// The closed month, as published in `MonthClosed`.
///
/// Money figures are the signed sums of the month's ledger entries in each
/// category (revenue positive, costs negative), so `net` is exactly the
/// month's change in cash. Energy figures are weighted by `days`.
/// `reliability` is this month's served share of demand (one when nothing
/// was demanded), not the company's rolling score.
#[derive(Debug, Clone, PartialEq)]
pub struct MonthlyReport {
    pub month: SimMonth,
    pub days: u32,
    pub revenue: Money,
    pub fuel: Money,
    pub start_up: Money,
    pub fixed_om: Money,
    pub construction: Money,
    pub net: Money,
    pub energy_by_kind: BTreeMap<PlantKind, MegawattHours>,
    /// Sorted by `(marginal_cost, plant)`.
    pub merit_order: Vec<MeritOrderEntry>,
    pub demand: MegawattHours,
    pub served: MegawattHours,
    pub unserved: MegawattHours,
    pub curtailed: MegawattHours,
    /// Signed `generation − served`, summed over the month.
    pub imbalance: MegawattHours,
    /// Surplus generation: the sum of each tick's positive imbalance (the
    /// deficits that `imbalance` nets off are left out), weighted by `days`.
    pub overgeneration: MegawattHours,
    pub peak_demand: Megawatts,
    pub reliability: Ratio,
    pub incidents: Vec<Incident>,
    /// Plants tripped by over-frequency protection (unweighted: the count of
    /// `OverFrequencyTrip` incidents of the played day).
    pub over_frequency_trips: u32,
    /// Share of the month's Normal-mode ticks whose frequency deviation
    /// exceeded the regulator's band (blackout ticks are excluded).
    pub out_of_band_share: Ratio,
    /// Whether `out_of_band_share` exceeded the regulator's allowed share:
    /// the month failed frequency performance (spec §14.4). Whether that
    /// drew a notice or a strike is told by `frequency_notice` and the
    /// `StrikeIssued` event.
    pub frequency_failed: bool,
    /// Whether the month drew a formal frequency notice instead of a strike.
    pub frequency_notice: bool,
    /// The annual rate review, in December only.
    pub rate_review: Option<RateReviewOutcome>,
}

/// The current representative day so far, unweighted.
///
/// Fed by the session: [`record_boundary_starts`](Self::record_boundary_starts)
/// and [`observe_plants`](Self::observe_plants) when the day begins,
/// [`add_tick`](Self::add_tick) and `observe_plants` after every tick, and
/// [`add_command_events`](Self::add_command_events) for manual commands.
#[derive(Debug, Clone, PartialEq)]
pub struct MonthAccumulator {
    /// Energy of every plant that appeared in a tick's `by_plant`.
    energy: BTreeMap<PlantId, MegawattHours>,
    /// `PlantStarting` events during the played day: recurring daily starts.
    daily_starts: BTreeMap<PlantId, u32>,
    /// One-off month-boundary starts reported when the day was initialised.
    boundary_starts: BTreeMap<PlantId, u32>,
    /// Plants that physically operated at any instant of the day.
    operated: BTreeSet<PlantId>,
    demand: MegawattHours,
    served: MegawattHours,
    unserved: MegawattHours,
    curtailed: MegawattHours,
    imbalance: MegawattHours,
    /// Σ max(0, imbalance) over the day's ticks.
    overgeneration: MegawattHours,
    peak_demand: Megawatts,
    incidents: Vec<Incident>,
    /// Ticks of the day spent in Normal grid mode.
    normal_ticks: u32,
    /// Normal-mode ticks whose deviation exceeded the frequency band.
    out_of_band_ticks: u32,
}

impl Default for MonthAccumulator {
    fn default() -> Self {
        Self {
            energy: BTreeMap::new(),
            daily_starts: BTreeMap::new(),
            boundary_starts: BTreeMap::new(),
            operated: BTreeSet::new(),
            demand: MegawattHours::ZERO,
            served: MegawattHours::ZERO,
            unserved: MegawattHours::ZERO,
            curtailed: MegawattHours::ZERO,
            imbalance: MegawattHours::ZERO,
            overgeneration: MegawattHours::ZERO,
            peak_demand: Megawatts::ZERO,
            incidents: Vec::new(),
            normal_ticks: 0,
            out_of_band_ticks: 0,
        }
    }
}

impl MonthAccumulator {
    /// Adds one tick: its energies (one minute of each power), the peak, the
    /// starts and incidents among `events`, and which plants produced.
    ///
    /// `deviation` is the tick's frequency deviation from nominal. A Normal
    /// mode tick counts as out of band when `|deviation| > band`; blackout
    /// ticks are in neither count (frequency is held at nominal in a
    /// blackout, which unserved energy already penalises).
    pub fn add_tick(
        &mut self,
        summary: &TickSummary,
        events: &[DomainEvent],
        deviation: Hertz,
        band: Hertz,
    ) {
        let minute = |mw: Megawatts| mw.energy_over_ticks(1);
        for (plant, output) in &summary.by_plant {
            *self.energy.entry(*plant).or_insert(MegawattHours::ZERO) += minute(*output);
            if *output > Megawatts::ZERO {
                self.operated.insert(*plant);
            }
        }
        self.demand += minute(summary.demand);
        self.served += minute(summary.served);
        self.unserved += minute(summary.unserved);
        self.curtailed += minute(summary.curtailed);
        self.imbalance += minute(summary.imbalance);
        self.overgeneration += minute(summary.imbalance.max(Megawatts::ZERO));
        self.peak_demand = self.peak_demand.max(summary.demand);
        if summary.mode == GridMode::Normal {
            self.normal_ticks += 1;
            if deviation.value().abs() > band.value() {
                self.out_of_band_ticks += 1;
            }
        }
        self.add_events(summary.tick, events);
    }

    /// Adds the events of a player command issued during tick `tick`: a
    /// manual start is a recurring daily start like a scheduled one.
    pub fn add_command_events(&mut self, tick: u32, events: &[DomainEvent]) {
        self.add_events(tick, events);
    }

    fn add_events(&mut self, tick: u32, events: &[DomainEvent]) {
        for event in events {
            let kind = match event {
                DomainEvent::PlantStarting { plant } => {
                    *self.daily_starts.entry(*plant).or_insert(0) += 1;
                    self.operated.insert(*plant);
                    continue;
                }
                DomainEvent::LoadShed { mw } => IncidentKind::LoadShed { mw: *mw },
                DomainEvent::Blackout => IncidentKind::Blackout,
                DomainEvent::BlackoutRestored => IncidentKind::BlackoutRestored,
                DomainEvent::OverFrequencyTrip { plant } => {
                    IncidentKind::OverFrequencyTrip { plant: *plant }
                }
                _ => continue,
            };
            self.incidents.push(Incident { tick, kind });
        }
    }

    /// Notes the plants physically running now (starting, online or
    /// stopping), connected or not.
    pub fn observe_plants<'a>(&mut self, plants: impl IntoIterator<Item = &'a Plant>) {
        let running = plants.into_iter().filter(|p| {
            matches!(
                p.status(),
                PlantStatus::Starting { .. }
                    | PlantStatus::Online { .. }
                    | PlantStatus::Stopping { .. }
            )
        });
        self.operated.extend(running.map(Plant::id));
    }

    /// Keeps the month-boundary starts reported when the day was initialised.
    pub fn record_boundary_starts(&mut self, starts: BTreeMap<PlantId, u32>) {
        self.boundary_starts = starts;
    }

    /// Clears everything for a new day.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Day energy per plant that was connected and commissioned at some tick.
    pub fn plant_energy(&self) -> &BTreeMap<PlantId, MegawattHours> {
        &self.energy
    }

    pub fn daily_starts(&self) -> &BTreeMap<PlantId, u32> {
        &self.daily_starts
    }

    pub fn boundary_starts(&self) -> &BTreeMap<PlantId, u32> {
        &self.boundary_starts
    }

    /// Whether `plant` physically operated at any point of the day.
    pub fn operated(&self, plant: PlantId) -> bool {
        self.operated.contains(&plant)
    }

    pub fn demand(&self) -> MegawattHours {
        self.demand
    }

    pub fn served(&self) -> MegawattHours {
        self.served
    }

    pub fn unserved(&self) -> MegawattHours {
        self.unserved
    }

    pub fn curtailed(&self) -> MegawattHours {
        self.curtailed
    }

    pub fn imbalance(&self) -> MegawattHours {
        self.imbalance
    }

    /// Σ max(0, imbalance) over the day's ticks, unweighted.
    pub fn overgeneration(&self) -> MegawattHours {
        self.overgeneration
    }

    pub fn peak_demand(&self) -> Megawatts {
        self.peak_demand
    }

    pub fn incidents(&self) -> &[Incident] {
        &self.incidents
    }

    /// How many plants over-frequency protection tripped during the day.
    pub fn over_frequency_trips(&self) -> u32 {
        let trips = self
            .incidents
            .iter()
            .filter(|i| matches!(i.kind, IncidentKind::OverFrequencyTrip { .. }))
            .count();
        u32::try_from(trips).unwrap_or(u32::MAX)
    }

    pub fn normal_ticks(&self) -> u32 {
        self.normal_ticks
    }

    pub fn out_of_band_ticks(&self) -> u32 {
        self.out_of_band_ticks
    }

    /// Out-of-band ticks over Normal-mode ticks; zero when there were none.
    pub fn out_of_band_share(&self) -> Ratio {
        if self.normal_ticks == 0 {
            return Ratio::ZERO;
        }
        let share = f64::from(self.out_of_band_ticks) / f64::from(self.normal_ticks);
        Ratio::new(share).unwrap_or(Ratio::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band() -> Hertz {
        Hertz::new(0.2)
    }

    fn summary(tick: u32, demand: f64, served: f64, by_plant: &[(u32, f64)]) -> TickSummary {
        let by_plant: BTreeMap<PlantId, Megawatts> = by_plant
            .iter()
            .map(|(id, mw)| (PlantId(*id), Megawatts::new(*mw)))
            .collect();
        let generation: Megawatts = by_plant.values().copied().sum();
        TickSummary {
            tick,
            demand: Megawatts::new(demand),
            served: Megawatts::new(served),
            unserved: Megawatts::new(demand - served),
            generation,
            curtailed: Megawatts::new(6.0),
            imbalance: generation - Megawatts::new(served),
            by_kind: BTreeMap::new(),
            by_plant,
            frequency: Hertz::new(60.0),
            mode: GridMode::Normal,
            shed: Ratio::ZERO,
        }
    }

    #[test]
    fn add_tick_accumulates_minutes_peak_starts_and_incidents() {
        let mut acc = MonthAccumulator::default();
        acc.add_tick(
            &summary(0, 120.0, 120.0, &[(1, 60.0), (2, 0.0)]),
            &[],
            Hertz::ZERO,
            band(),
        );
        let events = [
            DomainEvent::PlantStarting { plant: PlantId(2) },
            DomainEvent::LoadShed {
                mw: Megawatts::new(6.0),
            },
            DomainEvent::Blackout,
        ];
        acc.add_tick(
            &summary(1, 180.0, 120.0, &[(1, 60.0), (2, 30.0)]),
            &events,
            Hertz::ZERO,
            band(),
        );
        assert_eq!(acc.demand(), MegawattHours::new(5.0));
        assert_eq!(acc.served(), MegawattHours::new(4.0));
        assert_eq!(acc.unserved(), MegawattHours::new(1.0));
        assert_eq!(acc.curtailed(), MegawattHours::new(0.2));
        assert_eq!(acc.imbalance(), MegawattHours::new(-1.5));
        assert_eq!(acc.peak_demand(), Megawatts::new(180.0));
        assert_eq!(acc.plant_energy()[&PlantId(1)], MegawattHours::new(2.0));
        assert_eq!(acc.plant_energy()[&PlantId(2)], MegawattHours::new(0.5));
        assert_eq!(acc.daily_starts(), &BTreeMap::from([(PlantId(2), 1)]));
        assert!(acc.operated(PlantId(1)) && acc.operated(PlantId(2)));
        assert_eq!(
            acc.incidents(),
            &[
                Incident {
                    tick: 1,
                    kind: IncidentKind::LoadShed {
                        mw: Megawatts::new(6.0)
                    }
                },
                Incident {
                    tick: 1,
                    kind: IncidentKind::Blackout
                },
            ]
        );
        acc.reset();
        assert_eq!(acc, MonthAccumulator::default());
    }

    /// A tick in `mode` whose frequency is `deviation` away from 60 Hz.
    fn ticks_in(acc: &mut MonthAccumulator, mode: GridMode, deviations: &[f64]) {
        for (tick, deviation) in deviations.iter().enumerate() {
            let mut s = summary(tick as u32, 10.0, 10.0, &[]);
            s.mode = mode;
            acc.add_tick(&s, &[], Hertz::new(*deviation), band());
        }
    }

    #[test]
    fn out_of_band_counts_normal_mode_ticks_only() {
        let mut acc = MonthAccumulator::default();
        assert_eq!(acc.out_of_band_share(), Ratio::ZERO);
        // Normal: inside, inside, above, below (|dev| > 0.2): 2 of 4.
        ticks_in(&mut acc, GridMode::Normal, &[0.0, 0.2, 0.21, -0.5]);
        // Blackout ticks are excluded from both counts, even if deviating.
        ticks_in(&mut acc, GridMode::Blackout, &[5.0, -5.0, 0.0]);
        assert_eq!(acc.normal_ticks(), 4);
        assert_eq!(acc.out_of_band_ticks(), 2);
        assert_eq!(acc.out_of_band_share(), Ratio::new(0.5).unwrap());
        acc.reset();
        assert_eq!(acc.normal_ticks(), 0);
        assert_eq!(acc.out_of_band_ticks(), 0);
    }

    #[test]
    fn out_of_band_share_is_zero_without_normal_ticks() {
        let mut acc = MonthAccumulator::default();
        ticks_in(&mut acc, GridMode::Blackout, &[3.0, 3.0]);
        assert_eq!(acc.normal_ticks(), 0);
        assert_eq!(acc.out_of_band_share(), Ratio::ZERO);
    }

    #[test]
    fn overgeneration_sums_positive_imbalance_only() {
        let mut acc = MonthAccumulator::default();
        assert_eq!(acc.overgeneration(), MegawattHours::ZERO);
        // Surplus of 60 MW for two ticks, a deficit of 30 MW for one: only
        // the surplus counts, so 120 MW-minutes = 2 MWh.
        for (tick, generation) in [(0, 180.0), (1, 180.0), (2, 90.0)] {
            acc.add_tick(
                &summary(tick, 120.0, 120.0, &[(1, generation)]),
                &[],
                Hertz::ZERO,
                band(),
            );
        }
        assert_eq!(acc.overgeneration(), MegawattHours::new(2.0));
        // The signed total still nets the deficit off.
        assert_eq!(acc.imbalance(), MegawattHours::new(1.5));
        acc.reset();
        assert_eq!(acc.overgeneration(), MegawattHours::ZERO);
    }

    #[test]
    fn zero_output_alone_is_not_operation() {
        let mut acc = MonthAccumulator::default();
        acc.add_tick(
            &summary(0, 10.0, 10.0, &[(3, 0.0)]),
            &[],
            Hertz::ZERO,
            band(),
        );
        assert!(!acc.operated(PlantId(3)));
        assert_eq!(acc.plant_energy()[&PlantId(3)], MegawattHours::ZERO);
    }

    #[test]
    fn over_frequency_trip_is_an_incident() {
        let mut acc = MonthAccumulator::default();
        let plant = PlantId(3);
        acc.add_tick(
            &summary(4, 10.0, 10.0, &[(3, 0.0)]),
            &[
                DomainEvent::OverFrequencyTrip { plant },
                DomainEvent::PlantTripped { plant },
            ],
            Hertz::ZERO,
            band(),
        );
        assert_eq!(
            acc.incidents(),
            &[Incident {
                tick: 4,
                kind: IncidentKind::OverFrequencyTrip { plant }
            }]
        );
    }

    #[test]
    fn blackout_restoration_is_an_incident() {
        let mut acc = MonthAccumulator::default();
        acc.add_command_events(9, &[DomainEvent::Blackout, DomainEvent::BlackoutRestored]);
        assert_eq!(
            acc.incidents(),
            &[
                Incident {
                    tick: 9,
                    kind: IncidentKind::Blackout
                },
                Incident {
                    tick: 9,
                    kind: IncidentKind::BlackoutRestored
                },
            ]
        );
    }

    #[test]
    fn observe_plants_notes_every_physically_running_plant() {
        use crate::ids::{CellCoord, NodeId};
        use crate::region::PlantKind;
        use crate::region::fixtures::tiny_region;
        use crate::time::SimMonth;

        let region = tiny_region();
        let new = |id: u32, kind: PlantKind| {
            Plant::new_commissioned(
                PlantId(id),
                NodeId(id),
                CellCoord { x: 7, y: 3 },
                region.spec(kind).clone(),
                SimMonth::new(2027, 4).unwrap(),
            )
        };
        let offline = new(1, PlantKind::Coal);
        let mut starting = new(2, PlantKind::Coal);
        starting.start().unwrap();
        let mut online = new(3, PlantKind::Peaker);
        online.start().unwrap();
        while !online.is_online() {
            online.step(Megawatts::new(200.0));
        }
        let mut stopping = new(4, PlantKind::Peaker);
        stopping.start().unwrap();
        while !stopping.is_online() {
            stopping.step(Megawatts::new(200.0));
        }
        stopping.stop().unwrap();
        let under_construction = Plant::new_under_construction(
            PlantId(5),
            NodeId(5),
            CellCoord { x: 7, y: 3 },
            region.spec(PlantKind::Coal).clone(),
        );

        let mut acc = MonthAccumulator::default();
        acc.observe_plants([&offline, &starting, &online, &stopping, &under_construction]);
        let operated: Vec<bool> = (1..=5).map(|id| acc.operated(PlantId(id))).collect();
        assert_eq!(operated, [false, true, true, true, false]);
    }
}

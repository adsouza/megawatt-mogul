//! Closing the played month: accounting, construction, regulation and the
//! checks that can end the game.
//!
//! The representative day stands for every day of the month, so its energies
//! and recurring starts are weighted by `days_in_month`, while one-off
//! quantities (boundary starts, fixed O&M, instalments) are booked once.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::report::{MeritOrderEntry, MonthlyReport};
use super::{DayPhase, GameOverReason, GameSession, SessionStatus};
use crate::company::{AssetProfile, AssetRef, LedgerCategory, LedgerEntry, ReserveReviewInputs};
use crate::events::DomainEvent;
use crate::generation::PlantStatus;
use crate::grid::NodeKind;
use crate::ids::{CityId, PlantId};
use crate::region::PlantKind;
use crate::regulation::{
    Licence, RateReviewInputs, RateReviewOutcome, Regulator, compute_rate_review, reserve_excess,
};
use crate::time::SimMonth;
use crate::units::{MegawattHours, Megawatts, Money, Ratio};

/// What December's reserve review and rate review read from the session.
/// The real review and [`GameSession::rate_review_estimate`] both assemble it
/// with [`GameSession::review_inputs`], so the preview cannot drift from it.
pub(super) struct ReviewInputs {
    pub(super) reserve: ReserveReviewInputs,
    /// The rate review's inputs except the eligible rate base, which only the
    /// reserve review decides: read them through [`ReviewInputs::rate_review`].
    rate: RateReviewInputs,
}

impl ReviewInputs {
    /// The rate review's inputs on `eligible_rate_base`.
    pub(super) fn rate_review(self, eligible_rate_base: Money) -> RateReviewInputs {
        RateReviewInputs {
            eligible_rate_base,
            ..self.rate
        }
    }

    pub(super) fn firm_capacity(&self) -> Megawatts {
        self.rate.firm_capacity
    }

    /// The higher of the year's actual peak and the projected peak.
    pub(super) fn review_peak(&self) -> Megawatts {
        self.rate.tally.peak_demand.max(self.rate.projected_peak)
    }
}

impl GameSession {
    /// The review inputs on today's state, for a review held at the close of
    /// `month`: the projection of connected peak demand starts the month after.
    pub(super) fn review_inputs(&self, month: SimMonth) -> ReviewInputs {
        let params = self.region.regulator();
        let firm = self.power.firm_capacity(&params.storage_accreditation);
        let projected = self
            .power
            .projected_connected_peak(&self.region, month.next());
        let tally = self.regulator.tally().clone();
        let excess = reserve_excess(firm, tally.peak_demand, projected, params.reserve_margin);
        ReviewInputs {
            reserve: ReserveReviewInputs {
                year: month.year(),
                excess,
            },
            rate: RateReviewInputs {
                satisfaction: self.company.satisfaction(),
                tally,
                depreciation: self.year_depreciation,
                eligible_rate_base: Money::ZERO,
                firm_capacity: firm,
                projected_peak: projected,
                previous_tariff: self.regulator.tariff(),
                previous_cost_per_mwh: self.regulator.last_cost_per_mwh(),
            },
        }
    }

    /// Closes the played month and moves to `Review`, in order:
    ///
    /// 1. Ledger: revenue `tariff × served × days`; per plant, fuel
    ///    `marginal_cost × energy × days` and start-ups
    ///    `startup_cost × (daily_starts × days + boundary_starts)`; fixed O&M
    ///    for each commissioned plant; one construction instalment per asset
    ///    still under construction. Zero amounts are not booked.
    /// 2. Construction advances. Each commissioned plant or line enters the
    ///    rate base as new capital, and each plant's firm capacity is
    ///    reported to the regulator. Cities newly energised are reported.
    /// 3. One month of rate-base depreciation, added to the year's total.
    /// 4. The regulator tallies the month and judges its reliability; a
    ///    revoked licence ends the game. A month's out-of-band share is judged
    ///    with its unserved energy (spec §14.4).
    /// 5. The company's reliability window and satisfaction move.
    /// 6. In December, the reserve review and the annual rate review.
    /// 7. Three consecutive negative closes end the game in bankruptcy.
    /// 8. Each plant that was commissioned all month records whether it sat
    ///    offline, the report is published, the finished day is cleared, the
    ///    hydro schedule is frozen for review, and the phase moves to `Review`.
    pub(super) fn close_month(&mut self) -> Vec<DomainEvent> {
        let region = Arc::clone(&self.region);
        let month = self.clock.month();
        let days = month.days_in_month();
        let weight = f64::from(days);
        let mut events = Vec::new();

        let close = self.power.close_batteries();
        self.month_to_date
            .add_command_events(crate::time::TICKS_PER_DAY, &close);
        events.extend(close);
        let ledger_from = self.company.ledger().len();
        self.book_month(month, days, &mut events);
        self.book_makeup(month, &mut events);
        let commissioned = self.advance_construction(month, &mut events);
        self.close_geothermal(month, &commissioned, &mut events);
        self.year_depreciation += self.company.rate_base_mut().depreciate_month();

        let acc = &self.month_to_date;
        let demand = acc.demand() * weight;
        let served = acc.served() * weight;
        let unserved = acc.unserved() * weight;
        let peak = acc.peak_demand();
        let totals = LedgerTotals::of(&self.company.ledger()[ledger_from..]);

        let params = region.regulator();
        self.regulator
            .record_month(demand, served, totals.operating_cost(), peak);
        let out_of_band = acc.out_of_band_share();
        let verdict = self
            .regulator
            .assess_month(month, demand, unserved, out_of_band, params);
        let frequency_notice = verdict
            .iter()
            .any(|e| matches!(e, DomainEvent::FrequencyNoticeIssued { .. }));
        events.extend(verdict);
        if self.regulator.licence() == Licence::Revoked {
            self.end_game(GameOverReason::LicenseRevoked);
        }

        self.company.push_reliability(served, demand);
        let reliability = self.company.reliability();
        self.company.update_satisfaction(
            reliability,
            self.regulator.tariff(),
            params,
            region.satisfaction(),
        );

        let rate_review = (month.month() == 12).then(|| self.annual_review(month, &mut events));

        if self.company.close_solvency_month() && self.status == SessionStatus::Active {
            self.end_game(GameOverReason::Bankrupt);
            events.push(DomainEvent::Bankrupt);
        }

        self.record_offline_months(&commissioned);
        let report = self.build_report(month, days, totals, frequency_notice, rate_review);
        events.push(DomainEvent::MonthClosed(Box::new(report)));
        self.month_to_date.reset();
        self.day_ticks.clear();
        self.power.finish_hydro_day();
        events.push(self.change_phase(DayPhase::Review));
        events
    }

    fn end_game(&mut self, reason: GameOverReason) {
        if self.status == SessionStatus::Active {
            self.status = SessionStatus::GameOver(reason);
        }
    }

    /// Step 1: the month's cash movements.
    fn book_month(&mut self, month: SimMonth, days: u32, events: &mut Vec<DomainEvent>) {
        let acc = &self.month_to_date;
        let mut entries = vec![(
            LedgerCategory::Revenue,
            self.regulator.tariff().cost_of(acc.served(), days),
            None,
        )];
        for (id, energy) in acc.plant_energy() {
            let spec = self.power.plants()[id].spec();
            let fuel = spec.marginal_cost.cost_of(*energy, days);
            entries.push((LedgerCategory::Fuel, -fuel, Some(AssetRef::Plant(*id))));
        }
        let started: BTreeSet<PlantId> = acc
            .daily_starts()
            .keys()
            .chain(acc.boundary_starts().keys())
            .copied()
            .collect();
        for id in started {
            let daily = acc.daily_starts().get(&id).copied().unwrap_or(0);
            let boundary = acc.boundary_starts().get(&id).copied().unwrap_or(0);
            let starts = i64::from(daily) * i64::from(days) + i64::from(boundary);
            let cost = self.power.plants()[&id].spec().startup_cost * starts;
            entries.push((LedgerCategory::StartUp, -cost, Some(AssetRef::Plant(id))));
        }
        for plant in self.power.plants().values() {
            if !matches!(plant.status(), PlantStatus::UnderConstruction { .. }) {
                let om = plant.spec().fixed_om_per_month;
                entries.push((
                    LedgerCategory::FixedOm,
                    -om,
                    Some(AssetRef::Plant(plant.id())),
                ));
            }
        }
        for (asset, queue) in &mut self.instalments {
            if let Some(instalment) = queue.pop_front() {
                entries.push((LedgerCategory::Construction, -instalment, Some(*asset)));
            }
        }
        self.instalments.retain(|_, queue| !queue.is_empty());

        for (category, amount, subject) in entries {
            if amount != Money::ZERO {
                events.push(self.company.record(LedgerEntry {
                    month,
                    category,
                    amount,
                    subject,
                }));
            }
        }
    }

    /// Step 2: construction progress. Returns the plants commissioned now.
    fn advance_construction(
        &mut self,
        month: SimMonth,
        events: &mut Vec<DomainEvent>,
    ) -> BTreeSet<PlantId> {
        let connected_before = self.connected_cities();
        let building: Vec<PlantId> = self
            .power
            .plants()
            .values()
            .filter(|p| matches!(p.status(), PlantStatus::UnderConstruction { .. }))
            .map(|p| p.id())
            .collect();
        let mut commissioned = BTreeSet::new();
        for id in building {
            if !self.advance_exploration(id, events) {
                continue;
            }
            let plant = self.power.plant_mut(id).expect("plant listed above");
            let done = plant.advance_construction(month);
            if done.is_empty() {
                continue;
            }
            let spec = plant.spec();
            let firm = plant.firm_capacity(&self.region.regulator().storage_accreditation);
            self.company.rate_base_mut().add(
                AssetRef::Plant(id),
                spec.capital_cost,
                spec.book_life_months,
                month,
                AssetProfile::Plant {
                    firm,
                    node: plant.node(),
                },
            );
            self.regulator.record_firm_added(firm);
            commissioned.insert(id);
            events.extend(done);
        }

        let params = self.region.lines();
        for line_id in self.power.network_mut().advance_construction() {
            let line = *self
                .power
                .network()
                .line(line_id)
                .expect("network reports its own lines");
            let capital = self
                .line_construction(line_id)
                .expect("validated immutable line construction terms")
                .capital;
            self.company.rate_base_mut().add(
                AssetRef::Line(line_id),
                capital,
                params.book_life_months,
                month,
                AssetProfile::Line {
                    from: line.from,
                    to: line.to,
                },
            );
            events.push(DomainEvent::LineCommissioned { line: line_id });
        }

        for id in self.power.network_mut().advance_links() {
            let link = self.power.network().link(id).expect("commissioned link");
            let capital = self
                .link_construction(id)
                .expect("immutable link terms")
                .capital;
            self.company.rate_base_mut().add(
                AssetRef::Link(id),
                capital,
                self.region.hvdc().expect("link catalog").book_life_months,
                month,
                AssetProfile::Link {
                    from: link.from_grid(),
                    to: link.to_grid(),
                },
            );
        }
        self.power.recompute_islands();
        let connected_after = self.connected_cities();
        events.extend(
            connected_after
                .difference(&connected_before)
                .map(|city| DomainEvent::CityConnected { city: *city }),
        );
        commissioned
    }

    fn connected_cities(&self) -> BTreeSet<CityId> {
        let network = self.power.network();
        network
            .nodes()
            .filter_map(|(id, node)| match node.kind {
                NodeKind::City(city) if self.power.node_active(id) => Some(city),
                _ => None,
            })
            .collect()
    }

    /// Step 6: December's reserve review, then the rate review on the
    /// eligible rate base it leaves. Starts a fresh depreciation year.
    fn annual_review(
        &mut self,
        month: SimMonth,
        events: &mut Vec<DomainEvent>,
    ) -> RateReviewOutcome {
        let power = &self.power;
        let accreditation = &self.region.regulator().storage_accreditation;
        self.company.rate_base_mut().refresh_plant_firm(|id| {
            power
                .plant(id)
                .filter(|p| !matches!(p.status(), PlantStatus::UnderConstruction { .. }))
                .map(|p| p.firm_capacity(accreditation))
        });
        let utilisation = self.link_utilisation(month);
        events.extend(
            self.company
                .rate_base_mut()
                .review_link_utilisation(month.year(), &utilisation),
        );
        // Connectivity is already this close's: December's new cities count.
        let inputs = self.review_inputs(month);
        let capital = self
            .company
            .rate_base_mut()
            .apply_reserve_review(&inputs.reserve);
        events.extend(capital.into_iter().filter(|event| !is_zero_capital(event)));

        let rate_inputs = inputs.rate_review(self.company.rate_base().eligible_total());
        let outcome = compute_rate_review(&rate_inputs, self.region.regulator());
        events.extend(self.regulator.apply_rate_review(&outcome));
        self.year_depreciation = Money::ZERO;
        outcome
    }

    /// Step 8: remembers, for next month's boundary-start charge, whether each
    /// plant sat offline all month. Plants commissioned at this close keep the
    /// "returning from offline" state construction gave them.
    fn record_offline_months(&mut self, commissioned: &BTreeSet<PlantId>) {
        let ids: Vec<PlantId> = self.power.plants().keys().copied().collect();
        for id in ids {
            if commissioned.contains(&id) {
                continue;
            }
            let operated = self.month_to_date.operated(id);
            let plant = self.power.plant_mut(id).expect("plant listed above");
            if !matches!(plant.status(), PlantStatus::UnderConstruction { .. }) {
                plant.set_previous_month_offline(!operated);
            }
        }
    }

    fn build_report(
        &self,
        month: SimMonth,
        days: u32,
        totals: LedgerTotals,
        frequency_notice: bool,
        rate_review: Option<RateReviewOutcome>,
    ) -> MonthlyReport {
        let acc = &self.month_to_date;
        let weight = f64::from(days);
        let mut merit_order: Vec<MeritOrderEntry> = acc
            .plant_energy()
            .iter()
            .filter(|(id, _)| self.power.plants()[id].kind() != crate::region::PlantKind::Battery)
            .map(|(id, energy)| {
                let spec = self.power.plants()[id].spec();
                MeritOrderEntry {
                    plant: *id,
                    kind: spec.kind,
                    marginal_cost: spec.marginal_cost,
                    energy: *energy * weight,
                }
            })
            .collect();
        merit_order.sort_by(|a, b| {
            a.marginal_cost
                .value()
                .total_cmp(&b.marginal_cost.value())
                .then(a.plant.cmp(&b.plant))
        });
        let mut energy_by_kind = BTreeMap::<PlantKind, MegawattHours>::new();
        for entry in &merit_order {
            *energy_by_kind
                .entry(entry.kind)
                .or_insert(MegawattHours::ZERO) += entry.energy;
        }
        if !acc.storage().is_empty() {
            energy_by_kind.insert(
                crate::region::PlantKind::Battery,
                acc.storage().values().map(|b| b.output * weight).sum(),
            );
        }
        let demand = acc.demand() * weight;
        let served = acc.served() * weight;
        let out_of_band = acc.out_of_band_share();
        MonthlyReport {
            islands: acc.island_reports(weight),
            month,
            days,
            revenue: totals.revenue,
            fuel: totals.fuel,
            start_up: totals.start_up,
            fixed_om: totals.fixed_om,
            construction: totals.construction,
            net: totals.net(),
            storage: acc
                .storage()
                .iter()
                .map(|(id, total)| (*id, total.weighted(weight)))
                .collect(),
            energy_by_kind,
            merit_order,
            demand,
            served,
            unserved: acc.unserved() * weight,
            curtailed: acc.curtailed() * weight,
            curtailed_by_kind: acc
                .curtailed_by_kind()
                .iter()
                .map(|(kind, energy)| (*kind, *energy * weight))
                .collect(),
            imbalance: acc.imbalance() * weight,
            overgeneration: acc.overgeneration() * weight,
            rotating_outage: acc.rotating_outage() * weight,
            peak_outage_share: acc.peak_outage_share(),
            peak_demand: acc.peak_demand(),
            reliability: share(served, demand),
            incidents: acc.incidents().to_vec(),
            over_frequency_trips: acc.over_frequency_trips(),
            out_of_band_share: out_of_band,
            frequency_failed: Regulator::fails_frequency(out_of_band, self.region.regulator()),
            frequency_notice,
            rate_review,
        }
    }
}

/// `served / demand` as a ratio; one when nothing was demanded.
fn share(served: MegawattHours, demand: MegawattHours) -> Ratio {
    if demand.value() <= 0.0 {
        return Ratio::ONE;
    }
    Ratio::new((served.value() / demand.value()).clamp(0.0, 1.0)).unwrap_or(Ratio::ONE)
}

/// A reserve-review adjustment of no money, which would only be noise.
fn is_zero_capital(event: &DomainEvent) -> bool {
    matches!(
        event,
        DomainEvent::CapitalHeld { amount, .. }
            | DomainEvent::CapitalReleased { amount, .. }
            | DomainEvent::CapitalDisallowed { amount, .. }
            if *amount == Money::ZERO
    )
}

/// The month's ledger entries summed by category (signed as booked).
#[derive(Debug, Clone, Copy)]
struct LedgerTotals {
    revenue: Money,
    fuel: Money,
    start_up: Money,
    fixed_om: Money,
    construction: Money,
}

impl LedgerTotals {
    fn of(entries: &[LedgerEntry]) -> Self {
        let sum = |category| {
            entries
                .iter()
                .filter(|e| e.category == category)
                .map(|e| e.amount)
                .sum()
        };
        Self {
            revenue: sum(LedgerCategory::Revenue),
            fuel: sum(LedgerCategory::Fuel),
            start_up: sum(LedgerCategory::StartUp),
            fixed_om: sum(LedgerCategory::FixedOm),
            construction: sum(LedgerCategory::Construction),
        }
    }

    /// Prudent operating cost for the rate review: fuel, start-ups and fixed
    /// O&M, as a positive amount.
    fn operating_cost(&self) -> Money {
        -(self.fuel + self.start_up + self.fixed_om)
    }

    fn net(&self) -> Money {
        self.revenue + self.fuel + self.start_up + self.fixed_om + self.construction
    }
}

#[cfg(test)]
mod tests;

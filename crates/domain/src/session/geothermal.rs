//! Geothermal commands and the month-close reservoir lifecycle.
use super::{DayPhase, GameSession};
use crate::{
    company::{AssetRef, LedgerCategory, LedgerEntry},
    error::DomainError,
    events::DomainEvent,
    generation::{MakeupProject, MakeupQuote, PlantStatus},
    grid::NodeKind,
    ids::{CellCoord, NodeId, PlantId},
    region::{PlantKind, SitingRule},
    time::SimMonth,
    units::{Money, Ratio},
};
use std::collections::{BTreeMap, BTreeSet};

/// A removed project's immutable identity and unrecoverable spending.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrySite {
    pub plant: PlantId,
    pub node: NodeId,
    pub capital_lost: Money,
}
crate::state_record!(DrySite {
    plant,
    node,
    capital_lost
});

pub(super) fn makeup_capital(
    base: Money,
    effective: Money,
    authored: Money,
) -> Result<Money, DomainError> {
    let cost = if base == Money::ZERO {
        i128::from(authored.cents())
    } else {
        i128::from(authored.cents()) * i128::from(effective.cents()) / i128::from(base.cents())
    };
    Ok(Money::from_cents(
        i64::try_from(cost).map_err(|_| DomainError::ConstructionOverflow)?,
    ))
}

/// Round half-even at a fixed rational boundary; retain cents for large capital values.
pub(super) fn capital_share(capital: Money, ratio: Ratio) -> Money {
    let denominator = 1_000_000_000_000i128;
    let numerator = i128::from(capital.cents())
        * (ratio.value() * denominator as f64).round_ties_even() as i128;
    let (q, r) = (numerator / denominator, numerator % denominator);
    Money::from_cents(
        (q + i128::from(r * 2 > denominator || (r * 2 == denominator && q % 2 != 0))) as i64,
    )
}
impl GameSession {
    pub fn geothermal_available(&self) -> bool {
        self.geothermal_available
    }
    pub fn dry_cells(&self) -> &BTreeMap<CellCoord, DrySite> {
        &self.dry_cells
    }
    pub fn geothermal_site(&self, cell: CellCoord) -> Result<(Ratio, Ratio), DomainError> {
        self.plant_site_check(PlantKind::Geothermal, cell)
            .map_err(|reason| DomainError::InvalidSite {
                kind: PlantKind::Geothermal,
                cell,
                reason,
            })?;
        let params = self
            .region
            .geothermal()
            .expect("available site has geothermal parameters");
        let SitingRule::GeothermalSite { min } = self.region.spec(PlantKind::Geothermal).siting
        else {
            unreachable!("validated geothermal siting")
        };
        Ok(params.site(
            self.region
                .terrain()
                .get(cell)
                .expect("validated cell")
                .geothermal_potential,
            min,
        ))
    }
    pub(super) fn check_exploring_endpoint(&self, node: NodeId) -> Result<(), DomainError> {
        if let Some(n) = self.power.network().node(node)
            && let NodeKind::Plant(plant) = n.kind
            && self
                .power
                .plant(plant)
                .and_then(|p| p.geothermal())
                .is_some_and(|g| g.exploration_left > 0)
        {
            return Err(DomainError::ExplorationPending { plant });
        }
        Ok(())
    }
    pub fn makeup_quote(&self, plant: PlantId) -> Result<MakeupQuote, DomainError> {
        let p = self
            .power
            .plant(plant)
            .ok_or(DomainError::UnknownPlant { plant })?;
        let g = p
            .geothermal()
            .filter(|_| !matches!(p.status(), PlantStatus::UnderConstruction { .. }))
            .ok_or(DomainError::PlantNotGeothermal { plant })?;
        if g.makeup.is_some() {
            return Err(DomainError::GeothermalBusy { plant });
        }
        let params = self
            .region
            .geothermal()
            .expect("geothermal plant has params");
        let ceiling = g.ceiling(g.rounds + 1, params);
        let previous_ceiling = g.ceiling(g.rounds, params);
        if ceiling >= previous_ceiling
            || ceiling - g.field.value() < params.minimum_gain.value()
            || ceiling <= g.field.value()
        {
            return Err(DomainError::MakeupNotWorthwhile { plant });
        }
        let capital = makeup_capital(
            self.region.spec(PlantKind::Geothermal).capital_cost,
            p.spec().capital_cost,
            params.makeup_capital,
        )?;
        self.clock
            .month()
            .checked_add_months(params.makeup_months)?;
        self.company.rate_base().validate_addition(plant, capital)?;
        Ok(MakeupQuote {
            capital,
            months: params.makeup_months,
            from: p.available_capacity(),
            to: p.spec().capacity * ceiling,
            ceiling: Ratio::new(ceiling).expect("bounded ceiling"),
        })
    }
    pub fn drill_makeup_wells(&mut self, plant: PlantId) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        let q = self.makeup_quote(plant)?;
        let g = self
            .power
            .plant_mut(plant)
            .expect("quoted plant")
            .geothermal_mut()
            .expect("quoted geothermal");
        g.makeup = Some(MakeupProject {
            months_left: q.months,
            payments: q.capital.split_even(q.months).into(),
            capital: q.capital,
            ceiling: q.ceiling,
        });
        g.notice = None;
        Ok(vec![DomainEvent::MakeupDrillingOrdered { plant }])
    }
    pub fn dismiss_field_notice(
        &mut self,
        plant: PlantId,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        let g = self
            .power
            .plant_mut(plant)
            .and_then(|p| p.geothermal_mut())
            .filter(|g| g.notice.is_some())
            .ok_or(DomainError::NoFieldNotice { plant })?;
        g.notice = None;
        Ok(Vec::new())
    }
    /// Returns false only when a dry exploration removed the project.
    pub(super) fn advance_exploration(
        &mut self,
        id: PlantId,
        events: &mut Vec<DomainEvent>,
    ) -> bool {
        let p = self.power.plant_mut(id).expect("listed plant");
        let Some(g) = p.geothermal_mut() else {
            return true;
        };
        if g.exploration_left == 0 {
            return true;
        }
        g.exploration_left -= 1;
        if g.exploration_left > 0 {
            return true;
        }
        if self.rng.next_f64() < g.odds.value() {
            let params = self.region.geothermal().expect("geothermal params");
            let remaining =
                p.spec().capital_cost - p.geothermal().expect("geothermal").exploration_capital;
            self.instalments.insert(
                AssetRef::Plant(id),
                remaining
                    .split_even(p.spec().build_months - params.exploration_months)
                    .into(),
            );
            events.push(DomainEvent::ExplorationSucceeded { plant: id });
            true
        } else {
            let lost = g.exploration_capital;
            self.dry_cells.insert(
                p.cell(),
                DrySite {
                    plant: id,
                    node: p.node(),
                    capital_lost: lost,
                },
            );
            self.instalments.remove(&AssetRef::Plant(id));
            self.power.remove_dry_plant(id);
            events.push(DomainEvent::ExplorationDry {
                plant: id,
                capital_lost: lost,
            });
            false
        }
    }
    pub(super) fn book_makeup(&mut self, month: SimMonth, events: &mut Vec<DomainEvent>) {
        let ids: Vec<_> = self.power.plants().keys().copied().collect();
        for id in ids {
            let Some(m) = self
                .power
                .plant_mut(id)
                .and_then(|p| p.geothermal_mut())
                .and_then(|g| g.makeup.as_mut())
            else {
                continue;
            };
            if let Some(amount) = m.payments.pop_front()
                && amount != Money::ZERO
            {
                events.push(self.company.record(LedgerEntry {
                    month,
                    category: LedgerCategory::Construction,
                    amount: -amount,
                    subject: Some(AssetRef::Plant(id)),
                }));
            }
        }
    }
    pub(super) fn close_geothermal(
        &mut self,
        month: SimMonth,
        commissioned: &BTreeSet<PlantId>,
        events: &mut Vec<DomainEvent>,
    ) {
        let Some(params) = self.region.geothermal() else {
            return;
        };
        let ids: Vec<_> = self.power.plants().keys().copied().collect();
        for id in ids {
            let p = self.power.plant_mut(id).expect("listed plant");
            if commissioned.contains(&id)
                || matches!(p.status(), PlantStatus::UnderConstruction { .. })
            {
                continue;
            }
            let spec = p.spec().clone();
            let node = p.node();
            let Some(g) = p.geothermal_mut() else {
                continue;
            };
            g.decline(params);
            self.company
                .rate_base_mut()
                .decline_additions(id, params.monthly_decline);
            let done = if let Some(m) = g.makeup.as_mut() {
                m.months_left -= 1;
                m.months_left == 0
            } else {
                false
            };
            if done {
                let m = g.makeup.take().expect("completed project");
                let before = g.field;
                if m.ceiling > g.field {
                    g.field = m.ceiling;
                }
                g.rounds += 1;
                g.notice = None;
                let restored = spec.capacity * (g.field.value() - before.value());
                self.company.rate_base_mut().add_improvement(
                    id,
                    m.capital,
                    month,
                    spec.capacity_credit.of(restored),
                    node,
                );
                self.regulator
                    .record_firm_added(spec.capacity_credit.of(restored));
                events.push(DomainEvent::MakeupDrillingCompleted {
                    plant: id,
                    restored,
                });
            }
            for threshold in g.thresholds() {
                let first = !g.taught;
                g.taught = true;
                events.push(DomainEvent::FieldDeclineThreshold {
                    plant: id,
                    threshold,
                    first,
                });
            }
            p.refresh_resource_boundary();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        region::{RegionDefinition, fixtures::tiny_region_parts},
        session::GameSettings,
        units::Megawatts,
    };
    use std::sync::Arc;
    fn session(success: bool) -> GameSession {
        let mut p = tiny_region_parts();
        let g = p.geothermal.as_mut().unwrap();
        g.success_min = Ratio::new(if success { 1.0 } else { 0.0 }).unwrap();
        g.success_max = g.success_min;
        GameSession::new(
            Arc::new(RegionDefinition::try_from(p).unwrap()),
            7,
            GameSettings::default(),
        )
    }
    fn close(s: &mut GameSession) -> Vec<DomainEvent> {
        s.begin_operating().unwrap();
        let mut events = Vec::new();
        for _ in 0..1440 {
            events.extend(s.tick().unwrap());
        }
        s.finish_review().unwrap();
        events
    }
    const CELL: CellCoord = CellCoord { x: 4, y: 4 };
    #[test]
    fn geothermal_snapshot_rejects_invalid_stage_field_notice_and_makeup_payments() {
        let mut exploring = session(true);
        let (id, _) = exploring.build_plant(PlantKind::Geothermal, CELL).unwrap();
        let reject = |s: &GameSession| {
            assert!(
                GameSession::from_snapshot_state(&s.snapshot_state(), s.region.clone()).is_err()
            );
        };
        let mut bad = exploring.clone();
        bad.power
            .plant_mut(id)
            .unwrap()
            .geothermal_mut()
            .unwrap()
            .exploration_left = 0;
        reject(&bad);
        let mut bad = exploring.clone();
        bad.power
            .plant_mut(id)
            .unwrap()
            .geothermal_mut()
            .unwrap()
            .rounds = 1;
        reject(&bad);
        for _ in 0..12 {
            close(&mut exploring);
        }
        for fault in 0..4 {
            let mut bad = exploring.clone();
            let g = bad.power.plant_mut(id).unwrap().geothermal_mut().unwrap();
            match fault {
                0 => g.field = Ratio::ZERO,
                1 => g.rounds = u32::MAX,
                2 => g.notice = Some(90), // No decline has occurred at commissioning.
                _ => g.crossed = 512,
            }
            reject(&bad);
        }
        exploring
            .power
            .plant_mut(id)
            .unwrap()
            .geothermal_mut()
            .unwrap()
            .field = Ratio::new(0.1).unwrap();
        exploring.drill_makeup_wells(id).unwrap();
        let mut building = session(true);
        let (building_id, _) = building.build_plant(PlantKind::Geothermal, CELL).unwrap();
        building
            .power
            .plant_mut(building_id)
            .unwrap()
            .geothermal_mut()
            .unwrap()
            .makeup = exploring
            .power
            .plant(id)
            .unwrap()
            .geothermal()
            .unwrap()
            .makeup
            .clone();
        reject(&building);
        let mut bad = exploring.clone();
        let m = bad
            .power
            .plant_mut(id)
            .unwrap()
            .geothermal_mut()
            .unwrap()
            .makeup
            .as_mut()
            .unwrap();
        m.payments[0] += Money::from_cents(1);
        reject(&bad);
        let mut bad = exploring;
        bad.power
            .plant_mut(id)
            .unwrap()
            .geothermal_mut()
            .unwrap()
            .makeup
            .as_mut()
            .unwrap()
            .ceiling = Ratio::ONE;
        reject(&bad);
    }
    #[test]
    fn geothermal_exploration_success_commissions_at_twelve_closes_and_blocks_connections_until_success()
     {
        let mut s = session(true);
        let capital = s.region.spec(PlantKind::Geothermal).capital_cost;
        let (id, _) = s.build_plant(PlantKind::Geothermal, CELL).unwrap();
        let node = s.power.plant(id).unwrap().node();
        let before = s.snapshot_state();
        assert!(matches!(
            s.build_line(s.anchor, node),
            Err(DomainError::ExplorationPending { .. })
        ));
        assert_eq!(s.snapshot_state(), before);
        for _ in 0..2 {
            assert!(
                !close(&mut s)
                    .iter()
                    .any(|e| matches!(e, DomainEvent::ExplorationSucceeded { .. }))
            );
        }
        assert!(
            close(&mut s)
                .iter()
                .any(|e| matches!(e, DomainEvent::ExplorationSucceeded { plant } if *plant==id))
        );
        assert!(s.line_quote(s.anchor, node).is_ok());
        for _ in 3..11 {
            close(&mut s);
            assert!(matches!(
                s.power.plant(id).unwrap().status(),
                PlantStatus::UnderConstruction { .. }
            ));
        }
        close(&mut s);
        let p = s.power.plant(id).unwrap();
        assert!(p.auto_follow() && p.commitment().hours().iter().all(|h| *h));
        assert_eq!(
            p.geothermal().unwrap().field,
            p.geothermal().unwrap().initial_field
        );
        assert_eq!(
            s.company
                .rate_base()
                .asset(AssetRef::Plant(id))
                .unwrap()
                .original,
            capital
        );
        let paid: Money = s
            .company
            .ledger()
            .iter()
            .filter(|e| e.subject == Some(AssetRef::Plant(id)))
            .map(|e| -e.amount)
            .sum();
        assert_eq!(paid, capital);
        let region = s.region.clone();
        let restored = GameSession::from_snapshot_state(&s.snapshot_state(), region).unwrap();
        assert_eq!(s.snapshot_state(), restored.snapshot_state());
    }
    #[test]
    fn geothermal_dry_removes_project_but_only_blocks_geothermal_reuse() {
        let mut s = session(false);
        let (id, _) = s.build_plant(PlantKind::Geothermal, CELL).unwrap();
        let node = s.power.plant(id).unwrap().node();
        close(&mut s);
        close(&mut s);
        let events = close(&mut s);
        assert!(events.iter().any(|e| matches!(e, DomainEvent::ExplorationDry { plant, capital_lost } if *plant==id && capital_lost.cents()==1_800_000_000)));
        assert!(s.power.plant(id).is_none() && s.power.network().node(node).is_none());
        assert!(s.company.rate_base().asset(AssetRef::Plant(id)).is_none());
        for kind in PlantKind::ALL {
            assert_eq!(
                s.plant_site_check(kind, CELL),
                if kind == PlantKind::Geothermal {
                    Err(crate::region::SiteError::ExploredDry)
                } else {
                    s.region.site_check(kind, CELL)
                },
                "{kind:?}"
            );
        }
        assert!(s.ids.next_value() > node.0);
        assert!(s.check_invariants().is_ok());
        assert!(GameSession::from_snapshot_state(&s.snapshot_state(), s.region.clone()).is_ok());
        let dry = *s.dry_cells().get(&CELL).unwrap();
        s.plant_quote(PlantKind::Peaker, CELL).unwrap();
        let (replacement, _) = s.build_plant(PlantKind::Peaker, CELL).unwrap();
        assert!(replacement.0 > node.0);
        assert!(s.power.plant(replacement).unwrap().node().0 > node.0);
        assert_eq!(s.dry_cells().get(&CELL), Some(&dry));
        assert_eq!(
            s.plant_site_check(PlantKind::Solar, CELL),
            Err(crate::region::SiteError::Occupied)
        );
        assert_eq!(
            s.plant_site_check(PlantKind::Geothermal, CELL),
            Err(crate::region::SiteError::ExploredDry)
        );
        let mut restored =
            GameSession::from_snapshot_state(&s.snapshot_state(), s.region.clone()).unwrap();
        assert_eq!(restored.snapshot_state(), s.snapshot_state());
        assert_eq!(close(&mut restored), close(&mut s));
        assert_eq!(restored.snapshot_state(), s.snapshot_state());
        assert!(s.check_invariants().is_ok());
    }
    #[test]
    fn geothermal_makeup_declines_while_drilling_and_adds_only_new_capital() {
        let mut s = session(true);
        let (id, _) = s.build_plant(PlantKind::Geothermal, CELL).unwrap();
        for _ in 0..12 {
            close(&mut s);
        }
        let before = s
            .company
            .rate_base()
            .asset(AssetRef::Plant(id))
            .unwrap()
            .clone();
        let g = s.power.plant_mut(id).unwrap().geothermal_mut().unwrap();
        g.field = Ratio::new(g.initial_field.value() * 0.8).unwrap();
        g.notice = Some(90);
        let quote = s.makeup_quote(id).unwrap();
        let events = s.drill_makeup_wells(id).unwrap();
        assert!(matches!(
            events[0],
            DomainEvent::MakeupDrillingOrdered { .. }
        ));
        assert!(s.makeup_quote(id).is_err());
        let old = s.power.plant(id).unwrap().geothermal().unwrap().field;
        close(&mut s);
        assert!(s.power.plant(id).unwrap().geothermal().unwrap().field < old);
        let mut restored =
            GameSession::from_snapshot_state(&s.snapshot_state(), s.region.clone()).unwrap();
        for _ in 0..2 {
            assert_eq!(close(&mut s), close(&mut restored));
        }
        assert_eq!(s.snapshot_state(), restored.snapshot_state());
        let p = s.power.plant(id).unwrap();
        assert_eq!(p.geothermal().unwrap().rounds, 1);
        assert_eq!(p.available_capacity(), quote.to);
        let rb = s.company.rate_base().asset(AssetRef::Plant(id)).unwrap();
        assert_eq!(rb.original, before.original);
        assert_eq!(rb.original_total(), before.original + quote.capital);
        assert_eq!(rb.additions().len(), 1);
        assert!(p.available_capacity() < Megawatts::new(150.0));
        // A second round restores to a lower ceiling and adds a second dated component.
        let initial = p.geothermal().unwrap().initial_field;
        s.power
            .plant_mut(id)
            .unwrap()
            .geothermal_mut()
            .unwrap()
            .field = Ratio::new(initial.value() * 0.7).unwrap();
        let second = s.makeup_quote(id).unwrap();
        s.drill_makeup_wells(id).unwrap();
        for _ in 0..3 {
            close(&mut s);
        }
        let p = s.power.plant(id).unwrap();
        assert_eq!(p.geothermal().unwrap().rounds, 2);
        assert_eq!(p.available_capacity(), second.to);
        assert!(second.to < quote.to);
        let rb = s.company.rate_base().asset(AssetRef::Plant(id)).unwrap();
        assert_eq!(rb.additions().len(), 2);
        assert!(rb.additions()[1].added_on > rb.additions()[0].added_on);
        assert_eq!(
            rb.original_total(),
            before.original + quote.capital + second.capital
        );
    }
    #[test]
    fn geothermal_notice_dismissal_is_durable_and_rearms_only_after_restoration() {
        let mut g = crate::generation::GeothermalState {
            exploration_left: 0,
            odds: Ratio::ONE,
            exploration_capital: Money::ZERO,
            initial_field: Ratio::new(0.55).unwrap(),
            field: Ratio::new(0.55).unwrap(),
            rounds: 0,
            makeup: None,
            crossed: 0,
            notice: None,
            taught: false,
        };
        assert!(g.thresholds().is_empty());
        g.field = Ratio::new(0.494).unwrap();
        assert_eq!(g.thresholds(), vec![90]);
        g.notice = None;
        assert!(g.thresholds().is_empty());
        g.field = Ratio::new(0.53).unwrap();
        assert!(g.thresholds().is_empty());
        g.field = Ratio::new(0.494).unwrap();
        assert_eq!(g.thresholds(), vec![90]);
    }
    /// Resuming from a snapshot taken mid-exploration (RNG position, instalments and stage
    /// included) closes the remaining months exactly as uninterrupted play does.
    #[test]
    fn geothermal_resume_mid_exploration_matches_uninterrupted_play() {
        for seed in [7, 8, 9, 10] {
            let mut a = GameSession::new(
                Arc::new(RegionDefinition::try_from(tiny_region_parts()).unwrap()),
                seed,
                GameSettings::default(),
            );
            a.build_plant(PlantKind::Geothermal, CELL).unwrap();
            close(&mut a);
            let mut b =
                GameSession::from_snapshot_state(&a.snapshot_state(), a.region.clone()).unwrap();
            for _ in 0..3 {
                assert_eq!(close(&mut a), close(&mut b), "seed {seed}");
            }
            assert_eq!(a.snapshot_state(), b.snapshot_state(), "seed {seed}");
        }
    }
    #[test]
    fn geothermal_is_neither_variable_nor_storage() {
        let region = RegionDefinition::try_from(tiny_region_parts()).unwrap();
        let spec = region.spec(PlantKind::Geothermal);
        assert!(PlantKind::Geothermal.is_dispatchable() && !PlantKind::Geothermal.is_variable());
        assert!(spec.battery.is_none() && spec.kind != PlantKind::Battery);
    }
    proptest::proptest! {
        /// Odds and field quality stay within their configured bounds and rise with potential,
        /// for any valid parameters and siting minimum, not just the shipped fixture.
        #[test]
        fn geothermal_site_odds_and_quality_stay_in_bounds_and_rise_with_potential(
            odds in (0.0f64..=1.0, 0.0f64..=1.0), quality in (0.01f64..=1.0, 0.01f64..=1.0),
            minimum in 0.0f64..1.0, a in 0.0f64..=1.0, b in 0.0f64..=1.0,
        ) {
            let r = |v: f64| Ratio::new(v).unwrap();
            let mut params = crate::region::fixtures::geothermal_params();
            (params.success_min, params.success_max) = (r(odds.0.min(odds.1)), r(odds.0.max(odds.1)));
            (params.quality_min, params.quality_max) = (r(quality.0.min(quality.1)), r(quality.0.max(quality.1)));
            let (lo, hi) = (params.site(r(a.min(b)), r(minimum)), params.site(r(a.max(b)), r(minimum)));
            for (odds, quality) in [lo, hi] {
                proptest::prop_assert!(params.success_min <= odds && odds <= params.success_max);
                proptest::prop_assert!(params.quality_min <= quality && quality <= params.quality_max);
            }
            proptest::prop_assert!(lo.0 <= hi.0 && lo.1 <= hi.1);
        }
        /// Available capacity, minimum output and firm MW scale with the field; the commitment
        /// spec uses the commissioned field, which no decline lowers.
        #[test]
        fn geothermal_capacities_scale_with_the_field(initial in 0.15f64..=1.0, share in 0.001f64..=1.0) {
            let region = RegionDefinition::try_from(tiny_region_parts()).unwrap();
            let spec = region.spec(PlantKind::Geothermal).clone();
            let nameplate = spec.capacity.value();
            let mut p = crate::generation::Plant::new_commissioned(
                PlantId(1), crate::ids::NodeId(1), CELL, spec.clone(), crate::time::SimMonth::new(2027, 4).unwrap(),
            );
            let field = initial * share;
            p.set_geothermal(crate::generation::GeothermalState {exploration_left:0,odds:Ratio::ONE,exploration_capital:Money::ZERO,initial_field:Ratio::new(initial).unwrap(),field:Ratio::new(field).unwrap(),rounds:0,makeup:None,crossed:0,notice:None,taught:false});
            let close_to = |a: Megawatts, b: f64| (a.value() - b).abs() <= 1e-9 * nameplate;
            let available = nameplate * field;
            proptest::prop_assert!(close_to(p.available_capacity(), available));
            proptest::prop_assert!(close_to(p.min_output(), available * spec.min_stable.value()));
            proptest::prop_assert!(close_to(p.firm_capacity(&region.regulator().storage_accreditation), available * spec.capacity_credit.value()));
            proptest::prop_assert!(close_to(p.commitment_spec().capacity, nameplate * initial));
            proptest::prop_assert_eq!(p.control_spec().capacity, p.available_capacity());
        }
        #[test]
        fn geothermal_site_interpolation_and_monthly_decline_are_bounded(a in 0.3f64..=1.0, b in 0.3f64..=1.0, months in 0..1000usize) {
            let params = crate::region::fixtures::geothermal_params();
            let potential = |v| Ratio::new(v).unwrap();
            let (lo, hi) = (a.min(b), a.max(b));
            let l = params.site(potential(lo), potential(0.3)); let h = params.site(potential(hi), potential(0.3));
            proptest::prop_assert!(l.0 <= h.0 && l.1 <= h.1);
            let mut field = h.1.value();
            for _ in 0..months { let next = field*(1.0-params.monthly_decline.value()); proptest::prop_assert!(next>0.0 && next<=field); field=next; }
        }
    }
}

#[cfg(test)]
mod physical_contracts {
    use super::*;
    use crate::{
        region::{
            RegionDefinition,
            fixtures::{geothermal_acceptance_parts, tiny_region_parts, two_area_parts},
        },
        session::GameSettings,
        units::Megawatts,
    };
    use std::sync::Arc;
    fn close(s: &mut GameSession) -> Vec<DomainEvent> {
        s.begin_operating().unwrap();
        let mut events = Vec::new();
        for _ in 0..1440 {
            events.extend(s.tick().unwrap());
        }
        s.finish_review().unwrap();
        events
    }
    #[test]
    fn geothermal_live_capacity_minimum_dispatch_and_firm_mw_use_field_not_nameplate() {
        let mut s = GameSession::new(
            Arc::new(RegionDefinition::try_from(geothermal_acceptance_parts()).unwrap()),
            7,
            GameSettings::default(),
        );
        let id = s
            .power
            .plants()
            .values()
            .find(|p| p.kind() == PlantKind::Geothermal)
            .unwrap()
            .id();
        let quality = 0.15 + (0.8 - 0.3) / (1.0 - 0.3) * (1.0 - 0.15);
        let capacity = 150.0 * quality;
        let p = s.power.plant(id).unwrap();
        assert!((p.available_capacity().value() - capacity).abs() < 1e-12);
        assert!((p.min_output().value() - capacity * 0.5).abs() < 1e-12);
        assert!(
            (p.firm_capacity(&s.region.regulator().storage_accreditation)
                .value()
                - capacity * 0.95)
                .abs()
                < 1e-12
        );
        let other_firm: Megawatts = s
            .power
            .plants()
            .values()
            .filter(|p| p.id() != id)
            .map(|p| {
                p.spec()
                    .firm_capacity(&s.region.regulator().storage_accreditation)
            })
            .sum();
        assert!(
            (s.power
                .firm_capacity(&s.region.regulator().storage_accreditation)
                .value()
                - other_firm.value()
                - capacity * 0.95)
                .abs()
                < 1e-9
        );
        let before = s.snapshot_state();
        assert!(
            s.set_dispatch_target(id, Megawatts::new(capacity + 1.0))
                .is_err()
        );
        assert_eq!(s.snapshot_state(), before);
        s.set_auto_follow(id, false).unwrap();
        s.set_dispatch_target(id, Megawatts::new(capacity)).unwrap();
        s.begin_operating().unwrap();
        for _ in 0..1440 {
            s.tick().unwrap();
            assert!(s.power.plant(id).unwrap().output().value() <= capacity + 1e-9);
        }
        let p = s.power.plant(id).unwrap();
        assert_eq!(p.spec().capacity, Megawatts::new(150.0));
        assert!((p.available_capacity().value() - capacity * 0.96).abs() < 1e-12);
        assert!(p.initial_target() <= p.available_capacity());
        assert!(s.check_invariants().is_ok());
        assert!(GameSession::from_snapshot_state(&s.snapshot_state(), s.region.clone()).is_ok());
    }
    /// Makeup can restore capacity up to the commissioned level, which lengthens shutdown. A
    /// pattern accepted while the field is low must still fit then, so it is validated against
    /// commissioned capacity: 13 off-hours fit 105 MW (53 + 720 ticks) but not 135 MW (68 + 720).
    #[test]
    fn geothermal_commitment_fits_the_commissioned_capacity_makeup_can_restore() {
        let mut s = GameSession::new(
            Arc::new(RegionDefinition::try_from(geothermal_acceptance_parts()).unwrap()),
            7,
            GameSettings::default(),
        );
        let id = s
            .power
            .plants()
            .values()
            .find(|p| p.kind() == PlantKind::Geothermal)
            .unwrap()
            .id();
        let g = s.power.plant_mut(id).unwrap().geothermal_mut().unwrap();
        g.initial_field = Ratio::new(0.9).unwrap();
        g.field = Ratio::new(0.7).unwrap();
        let off = |hours: usize| std::array::from_fn(|h| h >= hours);
        let before = s.snapshot_state();
        assert!(matches!(
            s.commit_plant(id, off(13)),
            Err(DomainError::CommitmentInfeasible { .. })
        ));
        assert_eq!(s.snapshot_state(), before);
        s.commit_plant(id, off(14)).unwrap();
    }
    #[test]
    fn geothermal_exploration_draws_once_per_completion_in_id_order_and_never_on_queries() {
        let region = Arc::new(RegionDefinition::try_from(tiny_region_parts()).unwrap());
        let mut baseline = GameSession::new(region.clone(), 7, GameSettings::default());
        let mut s = GameSession::new(region, 7, GameSettings::default());
        let ids = [
            s.build_plant(PlantKind::Geothermal, CellCoord { x: 4, y: 4 })
                .unwrap()
                .0,
            s.build_plant(PlantKind::Geothermal, CellCoord { x: 5, y: 4 })
                .unwrap()
                .0,
        ];
        for _ in 0..2 {
            close(&mut baseline);
            close(&mut s);
            assert_eq!(baseline.rng, s.rng);
        }
        let before = s.snapshot_state();
        for _ in 0..3 {
            s.geothermal_site(CellCoord { x: 3, y: 4 }).unwrap();
            let _ = s.supply_forecast();
        }
        assert_eq!(before, s.snapshot_state());
        close(&mut baseline);
        let events = close(&mut s);
        let mut expected = baseline.rng.clone();
        for id in ids {
            let success = expected.next_f64() < 0.7;
            assert!(events.iter().any(|e| if success {
                matches!(e,DomainEvent::ExplorationSucceeded{plant} if *plant==id)
            } else {
                matches!(e,DomainEvent::ExplorationDry{plant,..} if *plant==id)
            }));
        }
        assert_eq!(s.rng, expected);
    }
    #[test]
    fn geothermal_exploring_nodes_refuse_hvdc_as_well_as_ac_quotes_without_state_change() {
        let mut parts = two_area_parts();
        parts.geothermal.as_mut().unwrap().success_min = Ratio::ONE;
        parts.geothermal.as_mut().unwrap().success_max = Ratio::ONE;
        let mut s = GameSession::new(
            Arc::new(RegionDefinition::try_from(parts).unwrap()),
            7,
            GameSettings::default(),
        );
        let id = s
            .build_plant(PlantKind::Geothermal, CellCoord { x: 4, y: 4 })
            .unwrap()
            .0;
        let node = s.power.plant(id).unwrap().node();
        let before = s.snapshot_state();
        assert!(matches!(
            s.build_link(s.anchor, node),
            Err(DomainError::ExplorationPending { .. })
        ));
        assert_eq!(before, s.snapshot_state());
        for _ in 0..3 {
            close(&mut s);
        }
        assert!(s.link_quote(s.anchor, node).is_ok());
    }
}

#[cfg(test)]
mod loss_properties {
    use super::*;
    use crate::{
        region::{RegionDefinition, fixtures::tiny_region_parts},
        session::GameSettings,
    };
    use std::sync::Arc;
    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(24))]
        #[test]
        fn geothermal_dry_spending_never_enters_depreciation_rate_base_or_customer_revenue_requirement(seed in 0u64..1_000_000) {
            let mut parts=tiny_region_parts();let g=parts.geothermal.as_mut().unwrap();g.success_min=Ratio::ZERO;g.success_max=Ratio::ZERO;
            let region=Arc::new(RegionDefinition::try_from(parts).unwrap());
            let mut baseline=GameSession::new(region.clone(),seed,GameSettings::default());let mut dry=GameSession::new(region,seed,GameSettings::default());
            let id=dry.build_plant(PlantKind::Geothermal,CellCoord{x:4,y:4}).unwrap().0;
            for _ in 0..3 {for s in [&mut baseline,&mut dry] {s.begin_operating().unwrap();for _ in 0..1440 {s.tick().unwrap();}s.finish_review().unwrap();}}
            proptest::prop_assert!(dry.power.plant(id).is_none());
            proptest::prop_assert_eq!(dry.company.rate_base().total(),baseline.company.rate_base().total());
            proptest::prop_assert_eq!(dry.year_depreciation,baseline.year_depreciation);
            proptest::prop_assert_eq!(dry.regulator.tally().operating_cost,baseline.regulator.tally().operating_cost);
            proptest::prop_assert_eq!(dry.rate_review_estimate(),baseline.rate_review_estimate());
            proptest::prop_assert_eq!(baseline.company.cash()-dry.company.cash(),Money::from_dollars(18_000_000));
        }
    }
}

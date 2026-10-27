//! HVDC orders and schedules; validate every fallible input before changing state.
use super::{DayPhase, GameSession};
use crate::{
    company::AssetRef,
    error::DomainError,
    events::DomainEvent,
    grid::LineStatus,
    ids::{LinkId, NodeId},
    region::LinkConstructionTerms,
};

impl GameSession {
    /// A read-only controller view with the selected island's fleet, clocks and incidents.
    pub fn island_session(&self, id: NodeId) -> Option<Self> {
        let power = self.power.scoped_island(id, &self.region)?;
        let day_ticks = self
            .day_ticks
            .last()
            .and_then(|s| {
                let row = s.islands.get(&id)?;
                let mut tick = s.clone();
                tick.demand = row.demand;
                tick.served = row.served;
                tick.unserved = row.unserved;
                tick.frequency = row.frequency;
                tick.mode = row.mode;
                tick.outage = row.outage;
                tick.shed = row.shed;
                tick.generation = row.generation;
                tick.by_kind = row.by_kind.clone();
                tick.by_plant
                    .retain(|plant, _| power.plant(*plant).is_some());
                tick.batteries
                    .retain(|plant, _| power.plant(*plant).is_some());
                tick.battery_charge = tick.batteries.values().map(|b| b.charge).sum();
                tick.battery_discharge = tick.batteries.values().map(|b| b.discharge).sum();
                tick.imbalance =
                    row.generation + row.imports - row.exports - row.served - tick.battery_charge;
                tick.islands.clear();
                Some(tick)
            })
            .into_iter()
            .collect();
        Some(Self {
            region: self.region.clone(),
            settings: self.settings,
            clock: self.clock,
            phase: self.phase,
            status: self.status,
            rng: self.rng.clone(),
            ids: self.ids.clone(),
            anchor: id,
            power,
            company: self.company.clone(),
            regulator: self.regulator.clone(),
            instalments: self.instalments.clone(),
            month_to_date: self.month_to_date.for_island(id),
            year_depreciation: self.year_depreciation,
            day_ticks,
        })
    }
    pub fn link_utilisation(
        &self,
        month: crate::time::SimMonth,
    ) -> std::collections::BTreeMap<LinkId, f64> {
        let Some(spec) = self.region.hvdc() else {
            return Default::default();
        };
        self.power
            .network()
            .links()
            .filter_map(|link| {
                let asset = self
                    .company
                    .rate_base()
                    .assets()
                    .find(|a| a.asset == AssetRef::Link(link.id))?;
                let months = if asset.added_on.year() == month.year() {
                    u32::from(month.month()).saturating_sub(u32::from(asset.added_on.month())) + 1
                } else {
                    u32::from(month.month())
                };
                let designed =
                    spec.rating_mw * spec.designed_hours_per_year * f64::from(months) / 12.0;
                Some((
                    link.id,
                    if designed > 0.0 {
                        if link.review_year == month.year() {
                            link.sent.value() / designed
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    },
                ))
            })
            .collect()
    }
    pub fn link_quote(
        &self,
        from: NodeId,
        to: NodeId,
    ) -> Result<LinkConstructionTerms, DomainError> {
        let length = self.line_length(from, to)?;
        let network = self.power.network();
        let a = network
            .node(from)
            .ok_or(DomainError::UnknownNode { node: from })?
            .cell;
        let b = network
            .node(to)
            .ok_or(DomainError::UnknownNode { node: to })?
            .cell;
        if self.region.area_at(a) == self.region.area_at(b) {
            return Err(DomainError::LinkWithinArea);
        }
        let quote = self.region.link_terms(a, b, length)?;
        self.clock.month().checked_add_months(quote.months)?;
        Ok(quote)
    }
    pub fn link_construction(&self, id: LinkId) -> Option<LinkConstructionTerms> {
        let network = self.power.network();
        let link = network.link(id)?;
        self.region
            .link_terms(
                network.node(link.from_grid())?.cell,
                network.node(link.to_grid())?.cell,
                link.length_cells,
            )
            .ok()
    }
    pub fn build_link(
        &mut self,
        from: NodeId,
        to: NodeId,
    ) -> Result<(LinkId, Vec<DomainEvent>), DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        let quote = self.link_quote(from, to)?;
        let mut ids = self.ids.clone();
        let link = ids.next();
        self.power.network_mut().add_link(
            link,
            from,
            to,
            LineStatus::UnderConstruction {
                months_left: quote.months,
            },
        )?;
        self.ids = ids;
        self.instalments.insert(
            AssetRef::Link(link),
            quote.capital.split_even(quote.months).into(),
        );
        Ok((link, Vec::new()))
    }
    pub fn cancel_link_construction(
        &mut self,
        link: LinkId,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        self.power.network_mut().cancel_link(link)?;
        self.instalments.remove(&AssetRef::Link(link));
        Ok(Vec::new())
    }
    fn check_link_values(&self, link: LinkId, values: &[f64]) -> Result<(), DomainError> {
        self.power
            .network()
            .link(link)
            .ok_or(DomainError::UnknownLink { link })?;
        let spec = self.region.hvdc().ok_or(DomainError::HvdcUnavailable)?;
        if values
            .iter()
            .any(|v| !v.is_finite() || v.abs() > spec.rating_mw)
        {
            return Err(DomainError::InvalidLinkTarget);
        }
        Ok(())
    }
    pub fn set_link_schedule(
        &mut self,
        link: LinkId,
        mw: [f64; 24],
    ) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        self.check_link_values(link, &mw)?;
        self.power
            .network_mut()
            .link_mut(link)
            .expect("validated link")
            .schedule = mw;
        Ok(Vec::new())
    }
    pub fn set_link_target(
        &mut self,
        link: LinkId,
        mw: f64,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Operating])?;
        self.check_link_values(link, &[mw])?;
        if self
            .power
            .network()
            .link(link)
            .expect("validated link")
            .status
            != LineStatus::InService
        {
            return Err(DomainError::LinkNotInService { link });
        }
        self.power
            .network_mut()
            .link_mut(link)
            .expect("validated link")
            .override_mw = Some(mw);
        self.power.generators_changed();
        Ok(Vec::new())
    }
    pub fn clear_link_override(&mut self, link: LinkId) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Operating])?;
        self.check_link_values(link, &[])?;
        let current = self.power.network().link(link).expect("validated link");
        if current.status != LineStatus::InService {
            return Err(DomainError::LinkNotInService { link });
        }
        if current.override_mw.is_none() {
            return Ok(Vec::new());
        }
        self.power
            .network_mut()
            .link_mut(link)
            .expect("validated link")
            .override_mw = None;
        self.power.generators_changed();
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ids::{CellCoord, CityId},
        region::{PlantKind, RegionDefinition, fixtures},
        session::GameSettings,
        units::Megawatts,
    };
    use std::sync::Arc;
    fn game() -> GameSession {
        GameSession::new(
            Arc::new(RegionDefinition::try_from(fixtures::two_area_parts()).unwrap()),
            7,
            GameSettings::default(),
        )
    }
    fn remote(s: &GameSession) -> NodeId {
        s.power.cities()[&CityId(3)].node()
    }
    #[test]
    fn cross_area_ac_and_same_area_dc_refusals_are_atomic() {
        let mut s = game();
        let hash = s.state_hash();
        let home = s.anchor;
        let far = remote(&s);
        assert_eq!(s.build_line(home, far), Err(DomainError::LineCrossesAreas));
        let near = s.power.cities()[&CityId(2)].node();
        assert_eq!(s.build_link(home, near), Err(DomainError::LinkWithinArea));
        assert_eq!(s.state_hash(), hash);
    }
    #[test]
    fn link_quote_payments_cancellation_and_parallel_links() {
        let mut s = game();
        let home = s.anchor;
        let far = remote(&s);
        let q = s.link_quote(home, far).unwrap();
        assert_eq!(
            q.converter_costs.iter().map(|c| c.cents()).sum::<i64>(),
            101
        );
        assert_eq!(q, s.link_quote(far, home).unwrap());
        let link = s.build_link(home, far).unwrap().0;
        assert_eq!(s.power.network().converter_stations().count(), 2);
        assert_eq!(
            s.pending_instalments(AssetRef::Link(link))
                .unwrap()
                .iter()
                .map(|m| m.cents())
                .sum::<i64>(),
            q.capital.cents()
        );
        assert!(s.build_link(home, far).is_ok());
        s.cancel_link_construction(link).unwrap();
        assert!(
            s.power
                .network()
                .converter_stations()
                .all(|station| station.id.link != link)
        );
        assert!(s.power.network().link(link).is_none());
        assert!(s.pending_instalments(AssetRef::Link(link)).is_none());
    }
    #[test]
    fn dc_ramps_and_blocks_without_a_synchronous_machine_and_resumes() {
        let mut s = game();
        let home = s.anchor;
        let far = remote(&s);
        let link = s.ids.next();
        s.power
            .network_mut()
            .add_link(link, home, far, LineStatus::InService)
            .unwrap();
        // Install a commissioned remote generator and join it only to the remote city.
        let plant = s.ids.next();
        let node = s.ids.next();
        s.power
            .network_mut()
            .add_node(
                node,
                crate::grid::Node {
                    kind: crate::grid::NodeKind::Plant(plant),
                    cell: CellCoord { x: 4, y: 2 },
                },
            )
            .unwrap();
        let line = s.ids.next();
        s.power
            .network_mut()
            .add_line(line, far, node, LineStatus::InService)
            .unwrap();
        let mut p = crate::generation::Plant::new_commissioned(
            plant,
            node,
            CellCoord { x: 4, y: 2 },
            s.region.spec(PlantKind::Peaker).clone(),
            s.clock.month(),
        );
        p.set_commitment(crate::generation::CommitmentSchedule::all_day());
        p.set_auto_follow(true).unwrap();
        s.power.insert_plant(p);
        s.set_link_schedule(link, [100.0; 24]).unwrap();
        s.power.recompute_islands();
        let a = s.activation_assessment(far).unwrap();
        s.activate_island(far, true, a.revision).unwrap();
        s.begin_operating().unwrap();
        s.tick().unwrap();
        let dc = s.power.network().link(link).unwrap();
        assert_eq!(dc.flow, Megawatts::new(20.0));
        assert!(!dc.blocked);
        assert_eq!(s.day_ticks.last().unwrap().islands.len(), 2);
        s.check_invariants().unwrap();
        s.stop_plant(plant).unwrap();
        for _ in 0..200 {
            s.tick().unwrap();
            if s.power.network().link(link).unwrap().blocked {
                break;
            }
        }
        assert!(s.power.network().link(link).unwrap().blocked);
        assert_eq!(s.power.network().link(link).unwrap().flow, Megawatts::ZERO);
    }
    #[test]
    fn commissioned_link_charges_exact_instalments_enters_rate_base_and_energises() {
        let mut s = game();
        let from = s.anchor;
        let to = remote(&s);
        let quote = s.link_quote(from, to).unwrap();
        let id = s.build_link(from, to).unwrap().0;
        assert!(!s.power.network().is_connected(to));
        for month in 0..quote.months {
            s.begin_operating().unwrap();
            for _ in 0..crate::time::TICKS_PER_DAY {
                s.tick().unwrap();
            }
            if month + 1 < quote.months {
                s.finish_review().unwrap();
            }
        }
        assert_eq!(
            s.power.network().link(id).unwrap().status,
            LineStatus::InService
        );
        assert!(s.power.network().is_connected(to));
        let asset = s
            .company
            .rate_base()
            .assets()
            .find(|a| a.asset == AssetRef::Link(id))
            .unwrap();
        assert_eq!(asset.original, quote.capital);
        let paid: i64 = s
            .company
            .ledger()
            .iter()
            .filter(|e| e.subject == Some(AssetRef::Link(id)))
            .map(|e| e.amount.cents())
            .sum();
        assert_eq!(paid, -quote.capital.cents());
        assert!(s.pending_instalments(AssetRef::Link(id)).is_none());
        let restored =
            GameSession::from_snapshot_state(&s.snapshot_state(), s.region.clone()).unwrap();
        assert_eq!(s.state_hash(), restored.state_hash());
    }

    #[test]
    fn utilisation_prorates_the_commissioning_month_and_resets_the_review_year() {
        use crate::{
            company::AssetProfile,
            time::SimMonth,
            units::{MegawattHours, Money},
        };
        let mut s = game();
        let far = remote(&s);
        let link = s.build_link(s.anchor, far).unwrap().0;
        s.company.rate_base_mut().add(
            AssetRef::Link(link),
            Money::from_cents(10001),
            120,
            SimMonth::new(2027, 11).unwrap(),
            AssetProfile::Link {
                from: s.anchor,
                to: far,
            },
        );
        let spec = s.region.hvdc().unwrap();
        // Quarter of the two-month designed volume: November counts in full.
        let sent = spec.rating_mw * spec.designed_hours_per_year * 2.0 / 12.0 / 4.0;
        let dc = s.power.network_mut().link_mut(link).unwrap();
        dc.sent = MegawattHours::new(sent);
        dc.review_year = 2027;
        let december = s.link_utilisation(SimMonth::new(2027, 12).unwrap());
        assert!((december[&link] - 0.25).abs() < 1e-12);
        s.company
            .rate_base_mut()
            .review_link_utilisation(2027, &december);
        let asset = s
            .company
            .rate_base()
            .assets()
            .find(|a| a.asset == AssetRef::Link(link))
            .unwrap();
        assert!((asset.held_slice().unwrap().fraction() - 0.75).abs() < 1e-9);
        assert_eq!(
            s.link_utilisation(SimMonth::new(2028, 1).unwrap())[&link],
            0.0
        );
        let dc = s.power.network_mut().link_mut(link).unwrap();
        dc.sent = MegawattHours::new(sent);
        dc.review_year = 2028;
        assert!((s.link_utilisation(SimMonth::new(2028, 1).unwrap())[&link] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn city_service_uses_its_own_islands_outage_instead_of_the_region_average() {
        use crate::{grid::NodeKind, units::Megawatts};
        let mut s = game();
        let far = remote(&s);
        let NodeKind::City(city) = s.power.network().node(far).unwrap().kind else {
            panic!("city")
        };
        let months = s.link_quote(s.anchor, far).unwrap().months;
        s.build_link(s.anchor, far).unwrap();
        for _ in 0..months {
            s.begin_operating().unwrap();
            for _ in 0..crate::time::TICKS_PER_DAY {
                s.tick().unwrap();
            }
            s.finish_review().unwrap();
        }
        // Established customer obligations survive complete loss of local generation.
        s.power.activate_service(far);
        s.begin_operating().unwrap();
        s.tick().unwrap();
        let tick = s.grid_snapshot().last_tick.unwrap();
        let remote_island = s
            .power
            .island_nodes()
            .iter()
            .find(|(_, nodes)| nodes.contains(&far))
            .unwrap()
            .0;
        assert_eq!(tick.islands[remote_island].served, Megawatts::ZERO);
        assert!(tick.served > Megawatts::ZERO);
        let view = s.city_detail(city).unwrap();
        assert!(view.demand_now.unwrap() > Megawatts::ZERO);
        assert_eq!(view.served_now, Some(Megawatts::ZERO));
    }

    proptest::proptest! {
        #[test]
        fn rejected_schedules_never_mutate(value in proptest::prelude::any::<f64>()) {
            let mut s=game();let home=s.anchor;let far=remote(&s);let link=s.build_link(home,far).unwrap().0;
            let before=s.state_hash();let result=s.set_link_schedule(link,[value;24]);
            if !value.is_finite() || value.abs()>200.0 {proptest::prop_assert!(result.is_err());proptest::prop_assert_eq!(before,s.state_hash());}
            else {proptest::prop_assert!(result.is_ok());}
        }
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use crate::{
        ids::CityId,
        region::{RegionDefinition, fixtures},
        session::GameSettings,
    };
    use std::sync::Arc;
    #[test]
    fn operating_islands_links_and_reports_restore_and_continue_identically() {
        let region = Arc::new(RegionDefinition::try_from(fixtures::two_area_parts()).unwrap());
        let mut a = GameSession::new(region.clone(), 7, GameSettings::default());
        let far = a.power.cities()[&CityId(3)].node();
        let link = a.ids.next();
        let home = a.anchor;
        a.power
            .network_mut()
            .add_link(link, home, far, LineStatus::InService)
            .unwrap();
        a.set_link_schedule(link, [100.0; 24]).unwrap();
        a.begin_operating().unwrap();
        for _ in 0..30 {
            a.tick().unwrap();
        }
        a.set_link_target(link, -50.0).unwrap();
        let mut b = GameSession::from_snapshot_state(&a.snapshot_state(), region).unwrap();
        assert_eq!(a.state_hash(), b.state_hash());
        for _ in 30..1440 {
            assert_eq!(a.tick(), b.tick());
        }
        assert_eq!(a.state_hash(), b.state_hash());
    }
}

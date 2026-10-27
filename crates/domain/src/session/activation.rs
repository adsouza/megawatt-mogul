//! Explicit enrolment of remote customers, separate from physical construction.

use super::{CapacityShortfallWarning, DayPhase, GameSession};
use crate::{
    error::DomainError,
    events::DomainEvent,
    ids::{LinkId, NodeId},
    state::State,
    units::Megawatts,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationBlocker {
    MissingSynchronous { island: NodeId, hours: [bool; 24] },
    UnavailableImport { link: LinkId, hours: [bool; 24] },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivationAssessment {
    pub island: NodeId,
    pub revision: u64,
    pub new_cities: u32,
    pub peak_demand: Megawatts,
    pub supply_at_peak: Megawatts,
    pub blockers: Vec<ActivationBlocker>,
    /// Includes exporting islands: imports cannot conceal a shortage in their source grid.
    pub shortfalls: BTreeMap<NodeId, CapacityShortfallWarning>,
}

impl GameSession {
    /// Portable plan identity, independent of UI selection or command-journal metadata.
    pub fn activation_revision(&self) -> u64 {
        self.clock
            .month()
            .state_bytes()
            .into_iter()
            .chain(self.power.state_bytes())
            .fold(super::FNV_OFFSET, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(super::FNV_PRIME)
            })
    }

    pub fn activation_assessment(
        &self,
        island: NodeId,
    ) -> Result<ActivationAssessment, DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        let nodes = self
            .power
            .island_nodes()
            .get(&island)
            .ok_or(DomainError::UnknownNode { node: island })?;
        if self.power.island_active(island) {
            return Err(DomainError::ActivationBlocked { island });
        }
        let mut proposed = self.power.clone();
        proposed.activate_service(island);
        let local = proposed
            .scoped_island(island, &self.region)
            .expect("assessed island");
        let forecast = local.supply_forecast(&self.region, self.clock.month());
        let mut blockers = Vec::new();
        let ready = proposed.planned_synchronous_hours(island);
        let mut missing = std::array::from_fn(|h| {
            !ready[h]
                && forecast.minutes[h * 60..(h + 1) * 60]
                    .iter()
                    .any(|p| p.demand > Megawatts::ZERO)
        });
        if !ready.iter().any(|ready| *ready) {
            missing[0] = true;
        }
        if missing.iter().any(|blocked| *blocked) {
            blockers.push(ActivationBlocker::MissingSynchronous {
                island,
                hours: missing,
            });
        }
        for link in proposed.network().links() {
            let plan = proposed
                .link_planning_forecast(link.id)
                .expect("existing link");
            let hours = std::array::from_fn(|h| {
                let incoming = (plan.to_island == Some(island) && link.schedule[h] > 0.0)
                    || (plan.from_island == Some(island) && link.schedule[h] < 0.0);
                incoming
                    && (!plan.available(h)
                        || !plan
                            .from_island
                            .is_some_and(|id| proposed.island_active(id))
                        || !plan.to_island.is_some_and(|id| proposed.island_active(id)))
            });
            if hours.iter().any(|blocked| *blocked) {
                blockers.push(ActivationBlocker::UnavailableImport {
                    link: link.id,
                    hours,
                });
            }
        }
        let peak = forecast
            .minutes
            .iter()
            .reduce(|first, next| {
                if next.demand > first.demand {
                    next
                } else {
                    first
                }
            })
            .expect("a day has samples");
        let shortfalls = proposed
            .island_nodes()
            .keys()
            .filter(|id| proposed.island_active(**id))
            .filter_map(|id| {
                let power = proposed.scoped_island(*id, &self.region)?;
                let supply = power.supply_forecast(&self.region, self.clock.month());
                Some((*id, Self::capacity_shortfall_for(&supply)?))
            })
            .collect();
        Ok(ActivationAssessment {
            island,
            revision: self.activation_revision(),
            new_cities: self
                .power
                .cities()
                .values()
                .filter(|c| nodes.contains(&c.node()))
                .count() as u32,
            peak_demand: peak.demand,
            supply_at_peak: peak.gross_supply,
            blockers,
            shortfalls,
        })
    }

    /// Errors leave the complete session unchanged. An override acknowledges only ordinary
    /// capacity shortages; it cannot bypass missing grid support or a changed plan.
    pub fn activate_island(
        &mut self,
        island: NodeId,
        accept_shortfall: bool,
        expected_revision: u64,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        let assessment = self.activation_assessment(island)?;
        if expected_revision != assessment.revision {
            return Err(DomainError::ActivationPlanChanged { island });
        }
        if !assessment.blockers.is_empty() {
            return Err(DomainError::ActivationBlocked { island });
        }
        if !assessment.shortfalls.is_empty() && !accept_shortfall {
            return Err(DomainError::ActivationShortfall { island });
        }
        let cities: Vec<_> = self
            .power
            .cities()
            .values()
            .filter(|c| self.power.island_nodes()[&island].contains(&c.node()))
            .map(|c| c.id())
            .collect();
        self.power.activate_service(island);
        Ok(cities
            .into_iter()
            .map(|city| DomainEvent::CityConnected { city })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::{
        grid::{Node, NodeKind},
        ids::*,
        region::{PlantKind, fixtures},
    };

    fn ready_with_load(load: f64) -> (GameSession, NodeId) {
        let mut parts = fixtures::two_area_parts();
        parts.cities[2].base_load = crate::units::Megawatts::new(load);
        let mut s = GameSession::new(
            Arc::new(RegionDefinition::try_from(parts).unwrap()),
            7,
            GameSettings::default(),
        );
        let far = s.power.cities()[&CityId(3)].node();
        let link = s.ids.next();
        s.power
            .network_mut()
            .add_link(link, s.anchor, far, LineStatus::InService)
            .unwrap();
        let plant = s.ids.next();
        let node = s.ids.next();
        let cell = CellCoord { x: 4, y: 2 };
        s.power
            .network_mut()
            .add_node(
                node,
                Node {
                    kind: NodeKind::Plant(plant),
                    cell,
                },
            )
            .unwrap();
        let line = s.ids.next();
        s.power
            .network_mut()
            .add_line(line, far, node, LineStatus::InService)
            .unwrap();
        let mut p = Plant::new_commissioned(
            plant,
            node,
            cell,
            s.region.spec(PlantKind::Peaker).clone(),
            s.clock.month(),
        );
        p.set_commitment(CommitmentSchedule::all_day());
        p.set_auto_follow(true).unwrap();
        s.power.insert_plant(p);
        s.power.recompute_islands();
        (s, far)
    }

    #[test]
    fn commissioning_a_source_does_not_automatically_enrol_remote_customers() {
        let (s, far) = ready_with_load(1000.0);
        assert!(!s.power.island_active(far));
        let pending = s.snapshot_state();
        let restored = GameSession::from_snapshot_state(&pending, s.region.clone()).unwrap();
        assert!(!restored.power.island_active(far));
        assert_eq!(restored.snapshot_state(), pending);
        assert!(
            !s.city_views()
                .iter()
                .find(|c| c.node == far)
                .unwrap()
                .connected
        );
    }

    #[test]
    fn activation_requires_acknowledgement_and_is_atomic_and_persistent() {
        let (mut s, far) = ready_with_load(1000.0);
        let a = s.activation_assessment(far).unwrap();
        assert!(a.blockers.is_empty());
        assert!(a.shortfalls.contains_key(&far));
        let before = s.snapshot_state();
        assert_eq!(
            s.activate_island(far, false, a.revision),
            Err(DomainError::ActivationShortfall { island: far })
        );
        assert_eq!(s.snapshot_state(), before);
        let events = s.activate_island(far, true, a.revision).unwrap();
        assert_eq!(events.len(), 1);
        assert!(s.power.island_active(far));
        let mut restored =
            GameSession::from_snapshot_state(&s.snapshot_state(), s.region.clone()).unwrap();
        assert!(restored.power.island_active(far));
        let plant = restored
            .power
            .plants()
            .values()
            .find(|p| restored.power.island_nodes()[&far].contains(&p.node()))
            .unwrap()
            .id();
        restored.commit_plant(plant, [false; 24]).unwrap();
        assert!(restored.power.island_active(far));
        assert!(
            restored
                .island_session(far)
                .unwrap()
                .hourly_forecast()
                .iter()
                .any(|p| p.mean > crate::units::Megawatts::ZERO)
        );
    }

    #[test]
    fn missing_support_and_stale_plans_cannot_be_overridden() {
        let (mut s, far) = ready_with_load(1.0);
        let a = s.activation_assessment(far).unwrap();
        assert!(a.shortfalls.is_empty());
        let plant = s
            .power
            .plants()
            .values()
            .find(|p| s.power.island_nodes()[&far].contains(&p.node()))
            .unwrap()
            .id();
        s.commit_plant(plant, [false; 24]).unwrap();
        let before = s.snapshot_state();
        assert_eq!(
            s.activate_island(far, true, a.revision),
            Err(DomainError::ActivationPlanChanged { island: far })
        );
        let blocked = s.activation_assessment(far).unwrap();
        assert!(!blocked.blockers.is_empty());
        assert_eq!(
            s.activate_island(far, true, blocked.revision),
            Err(DomainError::ActivationBlocked { island: far })
        );
        assert_eq!(s.snapshot_state(), before);
    }

    #[test]
    fn a_feasible_plan_activates_only_on_explicit_command_and_old_saves_keep_service() {
        let (mut s, far) = ready_with_load(1.0);
        let old = s.snapshot_state_schema(4);
        let legacy = GameSession::from_snapshot_state(&old, s.region.clone()).unwrap();
        assert!(legacy.power.island_active(far));
        assert_eq!(legacy.snapshot_state_schema(4), old);
        assert!(!s.power.island_active(far));
        let a = s.activation_assessment(far).unwrap();
        assert!(a.shortfalls.is_empty() && a.blockers.is_empty());
        s.activate_island(far, false, a.revision).unwrap();
        s.check_invariants().unwrap();
    }

    #[test]
    fn activation_counts_rated_imports_after_losses_and_checks_the_exporting_grid() {
        let (mut s, far) = ready_with_load(10_000.0);
        let link = s.power.network().links().next().unwrap().id;
        let rating = s.region.hvdc().unwrap().rating_mw;
        s.set_link_schedule(link, [rating; 24]).unwrap();
        let route = s.power.network().link(link).unwrap();
        let spec = s.region.hvdc().unwrap();
        let factor = 1.0
            - 2.0 * spec.converter_loss
            - spec.loss_per_cell * crate::region::whole_cells(route.length_cells) as f64;
        let expected = s.region.spec(PlantKind::Peaker).capacity.value() + rating * factor;
        let a = s.activation_assessment(far).unwrap();
        assert!(a.blockers.is_empty());
        assert!((a.supply_at_peak.value() - expected).abs() < 1e-8);
        assert!(a.shortfalls.contains_key(&far));
        assert!(!s.power.island_active(far));
        s.begin_operating().unwrap();
        s.tick().unwrap();
        s.check_invariants().unwrap();
        assert_eq!(
            s.power.network().link(link).unwrap().flow,
            crate::units::Megawatts::ZERO,
            "a ready generator and schedule cannot bypass explicit activation"
        );
        let (mut s, far) = ready_with_load(10_000.0);
        let link = s.power.network().links().next().unwrap().id;
        s.set_link_schedule(link, [rating; 24]).unwrap();
        let home: Vec<_> = s
            .power
            .plants()
            .values()
            .filter(|p| s.power.island_nodes()[&s.anchor].contains(&p.node()))
            .map(|p| p.id())
            .collect();
        for plant in home {
            s.commit_plant(plant, [false; 24]).unwrap();
        }
        let a = s.activation_assessment(far).unwrap();
        assert!(
            a.blockers
                .iter()
                .any(|b| matches!(b, super::ActivationBlocker::UnavailableImport { .. }))
        );
        assert!(a.shortfalls.contains_key(&s.anchor));
        let before = s.snapshot_state();
        assert!(matches!(
            s.activate_island(far, true, a.revision),
            Err(DomainError::ActivationBlocked { .. })
        ));
        assert_eq!(s.snapshot_state(), before);
        let source = s
            .power
            .plants()
            .values()
            .find(|p| {
                s.power.island_nodes()[&s.anchor].contains(&p.node())
                    && p.kind() == PlantKind::Peaker
            })
            .unwrap()
            .id();
        s.commit_plant(source, [true; 24]).unwrap();
        let a = s.activation_assessment(far).unwrap();
        assert!(
            a.blockers.is_empty(),
            "both terminals now have grid support"
        );
        assert!(a.shortfalls.contains_key(&s.anchor));
        assert_eq!(
            s.activate_island(far, false, a.revision),
            Err(DomainError::ActivationShortfall { island: far })
        );
    }

    proptest::proptest! {
        #[test]
        fn activation_preview_and_rejections_are_pure_and_success_is_durable(load in 1.0f64..2000.0, acknowledge in proptest::bool::ANY, stale in proptest::bool::ANY) {
            let (mut s, far) = ready_with_load(load);
            let before = s.snapshot_state();
            let a = s.activation_assessment(far).unwrap();
            proptest::prop_assert_eq!(s.snapshot_state(), before.clone());
            let result = s.activate_island(far, acknowledge, a.revision.wrapping_add(u64::from(stale)));
            if result.is_err() {
                proptest::prop_assert_eq!(s.snapshot_state(), before);
            } else {
                proptest::prop_assert!(acknowledge || a.shortfalls.is_empty());
                proptest::prop_assert!(!stale);
                let restored = GameSession::from_snapshot_state(&s.snapshot_state(), s.region.clone()).unwrap();
                proptest::prop_assert!(restored.power.island_active(far));
                proptest::prop_assert_eq!(restored.snapshot_state(), s.snapshot_state());
            }
        }
    }
}

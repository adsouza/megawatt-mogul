//! Pure capacity assessment of a proposed network expansion at its energisation dates.
use super::{CapacityShortfallWarning, DayPhase, GameSession};
use crate::{
    error::DomainError,
    generation::{CommitmentSchedule, Plant, PlantStatus},
    grid::{LineStatus, Network, PowerSystem},
    ids::{CityId, LineId, LinkId, NodeId},
    time::{SimMonth, TICKS_PER_DAY, TICKS_PER_HOUR},
    units::Megawatts,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionKind {
    Ac,
    Dc,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConnectionWarning {
    pub month: SimMonth,
    pub island: NodeId,
    pub cities: u32,
    pub added_peak: Megawatts,
    pub shortage: CapacityShortfallWarning,
}

impl GameSession {
    /// Assumes ready generators are committed all day; uses authored resources and the
    /// existing HVDC schedules. This query never advances time, RNG or the live network.
    pub fn connection_warning(
        &self,
        kind: ConnectionKind,
        from: NodeId,
        to: NodeId,
    ) -> Result<Option<ConnectionWarning>, DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        let mut proposed = self.power.network().clone();
        match kind {
            ConnectionKind::Ac => {
                let quote = self.line_quote(from, to)?;
                proposed.add_line(
                    LineId(self.ids.next_value()),
                    from,
                    to,
                    LineStatus::UnderConstruction {
                        months_left: quote.months,
                    },
                )?;
            }
            ConnectionKind::Dc => {
                let quote = self.link_quote(from, to)?;
                proposed.add_link(
                    LinkId(self.ids.next_value()),
                    from,
                    to,
                    LineStatus::UnderConstruction {
                        months_left: quote.months,
                    },
                )?;
            }
        }
        // A route can energise an entire pre-built component, or accelerate cities that
        // otherwise wait for a longer ordered path. Assess each actual energisation date.
        let mut arrivals = BTreeMap::<u32, BTreeSet<CityId>>::new();
        for city in self.power.cities().values() {
            let before = self.power.network().months_until_connected(city.node());
            if let Some(after) = proposed.months_until_connected(city.node())
                && before.is_none_or(|before| after < before)
            {
                arrivals.entry(after).or_default().insert(city.id());
            }
        }
        for (months, cities) in arrivals {
            let month = self.clock.month().checked_add_months(months)?;
            let before =
                self.connection_power(self.power.network().projected_after(months), months, month);
            let after = self.connection_power(proposed.projected_after(months), months, month);
            let baseline: Vec<_> = before
                .island_nodes()
                .iter()
                .map(|(id, nodes)| {
                    (
                        nodes,
                        before
                            .scoped_connection_island(*id, &self.region)
                            .expect("projected island")
                            .supply_forecast(&self.region, month),
                    )
                })
                .collect();
            let added_peak = (0..TICKS_PER_DAY)
                .map(|tick| {
                    crate::demand::forecast_demand(
                        self.power
                            .cities()
                            .values()
                            .filter(|c| cities.contains(&c.id())),
                        &self.region,
                        month,
                        tick,
                    )
                })
                .fold(Megawatts::ZERO, Megawatts::max);
            let mut worst: Option<ConnectionWarning> = None;
            for (id, nodes) in after.island_nodes() {
                let forecast = after
                    .scoped_connection_island(*id, &self.region)
                    .expect("projected island")
                    .supply_forecast(&self.region, month);
                let prior: Vec<_> = baseline
                    .iter()
                    .filter(|(old, _)| !old.is_disjoint(nodes))
                    .map(|(_, f)| f)
                    .collect();
                for (tick, point) in forecast.assessed_minutes() {
                    let original: Megawatts = prior
                        .iter()
                        .map(|f| {
                            (f.minutes[tick as usize].demand
                                - f.minutes[tick as usize].gross_supply)
                                .max(Megawatts::ZERO)
                        })
                        .sum();
                    let shortfall = point.demand - point.gross_supply;
                    // Ignore numerical noise when comparing otherwise identical islands.
                    if (shortfall - original).value() > 1e-6
                        && worst.is_none_or(|w| shortfall > w.shortage.shortfall)
                    {
                        worst = Some(ConnectionWarning {
                            month,
                            island: *id,
                            cities: cities.len() as u32,
                            added_peak,
                            shortage: CapacityShortfallWarning {
                                hour: (tick / TICKS_PER_HOUR) as u8,
                                demand: point.demand,
                                deliverable: point.gross_supply,
                                shortfall,
                            },
                        });
                    }
                }
            }
            if worst.is_some() {
                return Ok(worst);
            }
        }
        Ok(None)
    }

    fn connection_power(&self, network: Network, months: u32, month: SimMonth) -> PowerSystem {
        let plants = self
            .power
            .plants()
            .iter()
            .map(|(id, p)| {
                let mut next = match p.status() {
                    PlantStatus::UnderConstruction { months_left } if months_left <= months => {
                        Plant::new_commissioned(*id, p.node(), p.cell(), p.spec().clone(), month)
                    }
                    _ => p.clone(),
                };
                if !matches!(next.status(), PlantStatus::UnderConstruction { .. })
                    && next.kind().is_dispatchable()
                {
                    next.set_commitment(CommitmentSchedule::all_day());
                    next.set_auto_follow(true).expect("dispatchable generator");
                }
                (*id, next)
            })
            .collect();
        let mut power = PowerSystem::new(
            network,
            plants,
            self.power.cities().clone(),
            self.power.model(),
        );
        power.recompute_islands();
        let synchronous: BTreeSet<_> = power
            .island_nodes()
            .iter()
            .filter(|(_, nodes)| {
                power.plants().values().any(|p| {
                    nodes.contains(&p.node())
                        && p.kind().is_dispatchable()
                        && !matches!(p.status(), PlantStatus::UnderConstruction { .. })
                })
            })
            .map(|(id, _)| *id)
            .collect();
        let node_islands: BTreeMap<_, _> = power
            .island_nodes()
            .iter()
            .flat_map(|(id, nodes)| nodes.iter().map(|n| (*n, *id)))
            .collect();
        for link in power.network_mut().links_mut() {
            if ![link.from, link.to].iter().all(|node| {
                node_islands
                    .get(node)
                    .is_some_and(|id| synchronous.contains(id))
            }) {
                link.schedule = [0.0; 24];
            }
        }
        power
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ids::{CellCoord, CityId},
        region::{CityDef, PlantKind, RegionDefinition, fixtures},
        session::GameSettings,
    };
    use std::sync::Arc;

    fn game(edit: impl FnOnce(&mut crate::region::RegionDefinitionParts)) -> GameSession {
        let mut parts = fixtures::tiny_region_parts();
        edit(&mut parts);
        GameSession::new(
            Arc::new(RegionDefinition::try_from(parts).unwrap()),
            7,
            GameSettings::default(),
        )
    }
    fn city(s: &GameSession, id: u32) -> NodeId {
        s.city_views()
            .into_iter()
            .find(|c| c.id == CityId(id))
            .unwrap()
            .node
    }
    #[test]
    fn expansion_warning_includes_the_entire_new_component_and_is_pure() {
        let mut s = game(|p| {
            p.cities[2].base_load = Megawatts::new(3000.0);
            p.cities.push(CityDef {
                id: CityId(4),
                name_key: "city-extra".into(),
                cell: CellCoord { x: 4, y: 2 },
                base_load: Megawatts::new(2000.0),
            });
            p.city_locales.insert(CityId(4), p.city_locales[&CityId(3)]);
        });
        s.build_line(city(&s, 3), city(&s, 4)).unwrap();
        let before = s.snapshot_state();
        let warning = s
            .connection_warning(ConnectionKind::Ac, s.anchor_node(), city(&s, 3))
            .unwrap()
            .unwrap();
        assert_eq!(warning.cities, 2);
        assert!(warning.added_peak > Megawatts::new(4000.0));
        assert!(warning.shortage.shortfall > Megawatts::ZERO);
        assert_eq!(s.snapshot_state(), before);
        assert_eq!(
            s.connection_warning(ConnectionKind::Ac, s.anchor_node(), city(&s, 3))
                .unwrap(),
            Some(warning)
        );
    }
    #[test]
    fn covered_expansions_and_isolated_orders_do_not_warn() {
        let mut s = game(|p| {
            p.cities[2].base_load = Megawatts::new(1.0);
        });
        assert_eq!(
            s.connection_warning(ConnectionKind::Ac, s.anchor_node(), city(&s, 3))
                .unwrap(),
            None
        );
        let cell = CellCoord { x: 3, y: 3 };
        let plant = s.build_plant(PlantKind::Peaker, cell).unwrap().0;
        let node = s.power().plant(plant).unwrap().node();
        assert_eq!(
            s.connection_warning(ConnectionKind::Ac, city(&s, 3), node)
                .unwrap(),
            None
        );
    }
    #[test]
    fn generators_ready_by_connection_count_but_later_construction_does_not() {
        for months in [1, 24] {
            let mut s = game(|p| {
                p.cities[2].base_load = Megawatts::new(500.0);
                p.catalog
                    .iter_mut()
                    .find(|p| p.kind == PlantKind::Nuclear)
                    .unwrap()
                    .build_months = months;
            });
            let id = s
                .build_plant(PlantKind::Nuclear, CellCoord { x: 6, y: 3 })
                .unwrap()
                .0;
            s.build_line(s.anchor_node(), s.power().plant(id).unwrap().node())
                .unwrap();
            let warning = s
                .connection_warning(ConnectionKind::Ac, s.anchor_node(), city(&s, 3))
                .unwrap();
            assert_eq!(warning.is_some(), months == 24);
        }
    }
    #[test]
    fn a_new_dc_island_cannot_borrow_unrelated_regional_capacity() {
        let mut parts = fixtures::two_area_parts();
        parts.cities[2].base_load = Megawatts::new(1000.0);
        let s = GameSession::new(
            Arc::new(RegionDefinition::try_from(parts).unwrap()),
            7,
            GameSettings::default(),
        );
        let warning = s
            .connection_warning(ConnectionKind::Dc, s.anchor_node(), city(&s, 3))
            .unwrap()
            .unwrap();
        assert_eq!(warning.cities, 1);
        assert_eq!(warning.shortage.deliverable, Megawatts::ZERO);
    }

    #[test]
    fn future_backbones_and_hvdc_terminal_readiness_and_losses_are_respected() {
        for kind in [PlantKind::Solar, PlantKind::Peaker] {
            let mut parts = fixtures::two_area_parts();
            parts.cities[2].base_load = Megawatts::new(3000.0);
            parts
                .catalog
                .iter_mut()
                .find(|p| p.kind == kind)
                .unwrap()
                .build_months = 1;
            let mut s = GameSession::new(
                Arc::new(RegionDefinition::try_from(parts).unwrap()),
                7,
                GameSettings::default(),
            );
            let plant = s.build_plant(kind, CellCoord { x: 4, y: 3 }).unwrap().0;
            let node = s.power().plant(plant).unwrap().node();
            let months = s.link_quote(s.anchor_node(), node).unwrap().months;
            let link = s.build_link(s.anchor_node(), node).unwrap().0;
            let spec = s.region().hvdc().unwrap().clone();
            s.set_link_schedule(link, [spec.rating_mw; 24]).unwrap();
            let warning = s
                .connection_warning(ConnectionKind::Ac, node, city(&s, 3))
                .unwrap()
                .unwrap();
            assert_eq!(
                warning.month,
                s.clock().month().checked_add_months(months).unwrap()
            );
            assert_eq!(warning.cities, 1);
            let mut proposed = s.power().network().clone();
            proposed
                .add_line(
                    LineId(s.ids.next_value()),
                    node,
                    city(&s, 3),
                    LineStatus::InService,
                )
                .unwrap();
            let power = s.connection_power(proposed.projected_after(months), months, warning.month);
            let id = *power
                .island_nodes()
                .iter()
                .find(|(_, nodes)| nodes.contains(&node))
                .unwrap()
                .0;
            let forecast = power
                .scoped_island(id, &s.region)
                .unwrap()
                .supply_forecast(&s.region, warning.month);
            if kind == PlantKind::Solar {
                assert_eq!(
                    forecast.minutes[0].gross_supply,
                    Megawatts::ZERO,
                    "solar-only terminal cannot receive DC at night"
                );
            } else {
                let route = s.power.network().link(link).unwrap();
                let loss = 2.0 * spec.converter_loss
                    + crate::region::whole_cells(route.length_cells) as f64 * spec.loss_per_cell;
                let supplied =
                    s.region().spec(kind).capacity.value() + spec.rating_mw * (1.0 - loss);
                assert!((forecast.minutes[0].gross_supply.value() - supplied).abs() < 1e-8);
                assert!((warning.shortage.deliverable.value() - supplied).abs() < 1e-8);
            }
        }
    }

    proptest::proptest! {
        #[test]
        fn connection_preview_is_pure_and_ac_endpoint_order_invariant(load in 0.0f64..10000.0) {
            let s=game(|p|p.cities[2].base_load=Megawatts::new(load));
            let before=s.snapshot_state();
            let (a,b)=(s.anchor_node(),city(&s,3));
            let first=s.connection_warning(ConnectionKind::Ac,a,b).unwrap();
            let reversed=s.connection_warning(ConnectionKind::Ac,b,a).unwrap();
            proptest::prop_assert_eq!(first,reversed);
            proptest::prop_assert_eq!(before,s.snapshot_state());
            if let Some(w)=first {
                proptest::prop_assert!(w.shortage.shortfall>Megawatts::ZERO);
                proptest::prop_assert!(w.month>s.clock().month());
                proptest::prop_assert_eq!(w.cities,1);
            }
        }
    }
}

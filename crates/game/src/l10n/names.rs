//! Names for the things ids point at: a plant, a city, a line.
//!
//! Events and errors carry ids only. A [`Names`] snapshot of the session turns them into what the
//! player recognises ("Coal plant 7", "Denver"), and falls back to the bare id ("Plant #7") when
//! the session does not know it.

use std::collections::BTreeMap;

use domain::ids::{CityId, LineId, NodeId, PlantId};
use domain::region::PlantKind;
use domain::session::GameSession;

use super::localize::Localize;
use super::{FluentArg, Localizer};

/// Plant kinds and city names for the ids of one session, read through `GameSession`'s query
/// methods only.
pub struct Names<'a> {
    l10n: &'a Localizer,
    line_names: BTreeMap<LineId, String>,
    plants: BTreeMap<PlantId, PlantKind>,
    cities: BTreeMap<CityId, String>,
    /// What stands on each network node, for naming the ends of a line.
    nodes: BTreeMap<NodeId, NodeThing>,
}

/// What a network node is.
#[derive(Debug, Clone, Copy)]
enum NodeThing {
    Plant(PlantId),
    City(CityId),
}

impl<'a> Names<'a> {
    /// The electrical area plus its stable representative node, distinguishing split islands.
    pub fn island(&self, session: &GameSession, id: NodeId) -> String {
        let node = self.node(id);
        let area = session.power().network().node(id).and_then(|n| {
            session.region().areas().and_then(|areas| {
                areas
                    .areas
                    .iter()
                    .find(|area| area.id == session.region().area_at(n.cell))
            })
        });
        area.map_or(node.clone(), |area| {
            self.l10n.args(
                "forecast-island-name",
                &[
                    ("area", self.l10n.text(&area.name_key).into()),
                    ("node", node.into()),
                ],
            )
        })
    }

    /// A snapshot of the plants and cities of `session`.
    pub fn new(l10n: &'a Localizer, session: &GameSession) -> Self {
        let plant_views = session.plant_views();
        let city_views = session.city_views();
        let nodes = city_views
            .iter()
            .map(|city| (city.node, NodeThing::City(city.id)))
            .chain(
                plant_views
                    .iter()
                    .map(|plant| (plant.node, NodeThing::Plant(plant.id))),
            )
            .collect();
        let line_names = session
            .line_views()
            .iter()
            .filter_map(|line| {
                starter_line_name(session, line.from, line.to).map(|key| (line.id, key.to_owned()))
            })
            .collect();
        Self {
            line_names,
            nodes,
            l10n,
            plants: plant_views
                .into_iter()
                .map(|plant| (plant.id, plant.kind))
                .chain(
                    session
                        .dry_cells()
                        .values()
                        .map(|site| (site.plant, PlantKind::Geothermal)),
                )
                .collect(),
            cities: city_views
                .into_iter()
                .map(|city| (city.id, city.name_key))
                .collect(),
        }
    }

    /// Knows no ids: every plant and city is shown as its bare id. For text that has no session.
    pub fn empty(l10n: &'a Localizer) -> Self {
        Self {
            l10n,
            line_names: BTreeMap::new(),
            plants: BTreeMap::new(),
            cities: BTreeMap::new(),
            nodes: BTreeMap::new(),
        }
    }

    /// `Coal plant 7`, or `Plant #7` if the session has no such plant.
    pub fn plant(&self, id: PlantId) -> String {
        match self.plants.get(&id) {
            Some(kind) => self.plant_as(id, *kind),
            None => self.l10n.args(
                "name-plant-unknown",
                &[("id", FluentArg::from(id.0.to_string()))],
            ),
        }
    }

    /// `Coal plant 7` for a plant whose kind the caller already knows (an event that carries it).
    pub fn plant_as(&self, id: PlantId, kind: PlantKind) -> String {
        self.l10n.args(
            "name-plant",
            &[
                ("kind", kind.localize(self.l10n).into()),
                ("id", id.0.to_string().into()),
            ],
        )
    }

    /// The city's name, or `City #3` if the session has no such city.
    pub fn city(&self, id: CityId) -> String {
        match self.cities.get(&id) {
            Some(name_key) => self.l10n.text(name_key),
            None => self
                .l10n
                .args("name-city-unknown", &[("id", id.0.to_string().into())]),
        }
    }

    pub fn converter(&self, station: domain::grid::ConverterStation) -> String {
        let end = match station.id.end {
            domain::grid::ConverterEnd::From => "A",
            domain::grid::ConverterEnd::To => "B",
        };
        self.l10n.args(
            "converter-station-name",
            &[
                ("id", format!("{}{}", station.id.link.0, end).into()),
                ("grid", self.node(station.grid_node).into()),
            ],
        )
    }

    /// The name of a network node: a city's name or `Coal plant 7`; a node the
    /// session does not know is `Grid point 12`.
    pub fn node(&self, id: NodeId) -> String {
        match self.nodes.get(&id) {
            Some(NodeThing::Plant(plant)) => self.plant(*plant),
            Some(NodeThing::City(city)) => self.city(*city),
            None => self
                .l10n
                .args("name-node-unknown", &[("id", id.0.to_string().into())]),
        }
    }

    /// The authored starter-link name, or `Line 4` for an unnamed line.
    pub fn line(&self, id: LineId) -> String {
        self.line_names.get(&id).map_or_else(
            || {
                self.l10n
                    .args("name-line", &[("id", id.0.to_string().into())])
            },
            |key| self.l10n.text(key),
        )
    }
}

/// Authored names apply to the starter home-to-city link, never a new branch to that city.
pub(crate) fn starter_line_name(session: &GameSession, from: NodeId, to: NodeId) -> Option<&str> {
    let home = session.anchor_node();
    let target = if from == home {
        to
    } else if to == home {
        from
    } else {
        return None;
    };
    let city = session
        .city_views()
        .into_iter()
        .find(|city| city.node == target)?;
    session.region().start_line_name(city.id)
}

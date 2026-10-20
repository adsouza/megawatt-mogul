//! Names for the things ids point at: a plant, a city, a line.
//!
//! Events and errors carry ids only. A [`Names`] snapshot of the session turns them into what the
//! player recognises ("Coal plant 7", "Denver"), and falls back to the bare id ("Plant #7") when
//! the session does not know it.

use std::collections::BTreeMap;

use domain::ids::{CityId, LineId, PlantId};
use domain::region::PlantKind;
use domain::session::GameSession;

use super::localize::Localize;
use super::{FluentArg, Localizer};

/// Plant kinds and city names for the ids of one session, read through `GameSession`'s query
/// methods only.
pub struct Names<'a> {
    l10n: &'a Localizer,
    plants: BTreeMap<PlantId, PlantKind>,
    cities: BTreeMap<CityId, String>,
}

impl<'a> Names<'a> {
    /// A snapshot of the plants and cities of `session`.
    pub fn new(l10n: &'a Localizer, session: &GameSession) -> Self {
        Self {
            l10n,
            plants: session
                .plant_views()
                .into_iter()
                .map(|plant| (plant.id, plant.kind))
                .collect(),
            cities: session
                .city_views()
                .into_iter()
                .map(|city| (city.id, city.name_key))
                .collect(),
        }
    }

    /// Knows no ids: every plant and city is shown as its bare id. For text that has no session.
    pub fn empty(l10n: &'a Localizer) -> Self {
        Self {
            l10n,
            plants: BTreeMap::new(),
            cities: BTreeMap::new(),
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

    /// `Line 4`. The session offers no query for a line's ends, so there is nothing more to say.
    pub fn line(&self, id: LineId) -> String {
        self.l10n
            .args("name-line", &[("id", id.0.to_string().into())])
    }
}

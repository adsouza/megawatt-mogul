//! The incident ticker (spec §8.4): what protection and the rotating outages did today, newest
//! first, each with its time of day. Every non-zero outage order is an incident, so the orders
//! are listed too.

use application::Game;
use bevy_egui::egui;
use domain::session::Incident;

use super::action::UiAction;
use crate::l10n::{LocalizeWith, Localizer, Names, format};

/// The most incidents the ticker lists.
pub const MAX_INCIDENTS: usize = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct IncidentsModel {
    pub title: String,
    /// `06:12: Protection shed 300 MW of load ...`, newest first.
    pub rows: Vec<String>,
    /// Shown when nothing has happened yet.
    pub empty: Option<String>,
}

pub fn incidents_model(game: &Game, l10n: &Localizer) -> IncidentsModel {
    let session = game.session();
    let incidents = session.incidents_today();
    // Names reads the network; build it only when there is something to name.
    let names = if incidents.is_empty() {
        Names::empty(l10n)
    } else {
        Names::new(l10n, session)
    };
    incidents_for(incidents, &names, l10n)
}

/// The ticker for `incidents`, oldest first as the day recorded them.
pub fn incidents_for(incidents: &[Incident], names: &Names, l10n: &Localizer) -> IncidentsModel {
    let rows: Vec<String> = incidents
        .iter()
        .rev()
        .take(MAX_INCIDENTS)
        .map(|incident| {
            l10n.args(
                "incident-row",
                &[
                    ("time", format::clock(incident.tick).into()),
                    ("incident", incident.kind.localize_with(l10n, names).into()),
                ],
            )
        })
        .collect();
    IncidentsModel {
        title: l10n.text("incidents-title"),
        empty: rows.is_empty().then(|| l10n.text("incidents-none")),
        rows,
    }
}

/// The heading and the incidents. It asks for nothing.
pub fn draw_incidents(ui: &mut egui::Ui, model: &IncidentsModel) -> Vec<UiAction> {
    ui.strong(model.title.as_str());
    egui::ScrollArea::vertical()
        .id_salt("operating_incidents")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(empty) = &model.empty {
                ui.weak(empty.as_str());
            }
            for row in &model.rows {
                ui.label(row.as_str());
            }
        });
    Vec::new()
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};
    use domain::ids::PlantId;
    use domain::session::IncidentKind;
    use domain::units::{Megawatts, Ratio};

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    #[test]
    fn incident_ticker_orders_newest_first() {
        let l10n = shipped_l10n();
        let names = Names::empty(&l10n);
        let incidents = [
            Incident {
                tick: 0,
                kind: IncidentKind::RotatingOutage {
                    share: Ratio::new(0.12).unwrap(),
                },
            },
            Incident {
                tick: 372,
                kind: IncidentKind::LoadShed {
                    mw: Megawatts::new(300.0),
                },
            },
            Incident {
                tick: 372,
                kind: IncidentKind::Blackout,
            },
            Incident {
                tick: 1_030,
                kind: IncidentKind::OverFrequencyTrip { plant: PlantId(7) },
            },
        ];

        let model = incidents_for(&incidents, &names, &l10n);

        let text = |incident: &Incident| incident.kind.localize_with(&l10n, &names);
        assert_eq!(
            model.rows,
            [
                format!("17:10: {}", text(&incidents[3])),
                format!("06:12: {}", text(&incidents[2])),
                format!("06:12: {}", text(&incidents[1])),
                format!("00:00: {}", text(&incidents[0])),
            ]
        );
        assert!(model.rows[3].contains("12.0%"), "{}", model.rows[3]);
        assert_eq!(model.title, "Incidents");
        assert_eq!(model.empty, None);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_ticker_keeps_the_latest_and_says_when_there_are_none() {
        let l10n = shipped_l10n();
        let names = Names::empty(&l10n);
        let many: Vec<Incident> = (0..MAX_INCIDENTS as u32 + 5)
            .map(|tick| Incident {
                tick,
                kind: IncidentKind::BlackoutRestored,
            })
            .collect();

        let model = incidents_for(&many, &names, &l10n);

        assert_eq!(model.rows.len(), MAX_INCIDENTS);
        assert!(model.rows[0].starts_with("00:24"), "{}", model.rows[0]);
        let none = incidents_for(&[], &names, &l10n);
        assert!(none.rows.is_empty());
        assert_eq!(none.empty.as_deref(), Some("No incidents today."));
    }

    #[test]
    fn the_model_reads_the_days_incidents() {
        let l10n = shipped_l10n();
        // Nothing committed: the day opens in a rotating outage, the first incident.
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();
        assert!(!game.session().incidents_today().is_empty());

        let model = incidents_model(&game, &l10n);

        let names = Names::new(&l10n, game.session());
        assert_eq!(
            model,
            incidents_for(game.session().incidents_today(), &names, &l10n)
        );
        assert!(
            model
                .rows
                .last()
                .unwrap()
                .starts_with("00:00: Rotating outage")
        );
    }
}

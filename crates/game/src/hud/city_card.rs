//! The city card (spec §16.8): a selected city's name, whether it is connected, and its demand.
//!
//! It shows wherever a plant's details show: in Planning's inspector, and in Operating's
//! selected-object block above the plant list, which keeps its fixed height for it. A connected
//! city's figures are the system's scaled by its share of the connected base load
//! ([`domain::session::CityDetail`]); an unconnected one shows the demand it would add.

use application::Game;
use bevy_egui::egui;
use domain::ids::CityId;
use domain::time::TICKS_PER_HOUR;
use domain::units::{Megawatts, Ratio};

use super::top_bar::Stat;
use crate::l10n::{Localizer, Names, format};

/// The most figures a city card shows: base load, the peak, and in Operating the demand now, the
/// load served now and the share cut.
pub const MOST_CITY_STATS: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct CityCardModel {
    pub city: CityId,
    /// The city's localised name.
    pub name: String,
    /// `Connected` or `Not connected`.
    pub connection: String,
    pub connected: bool,
    /// Base load and the peak; in Operating, for a connected city, the demand and the load served
    /// now, and the share cut while an outage or a shed is on.
    pub stats: Vec<Stat>,
    /// How to connect an unconnected city; `None` for a connected one.
    pub hint: Option<String>,
}

/// The card for `city`; `None` for an id the session doesn't know.
pub fn city_card_model(game: &Game, l10n: &Localizer, city: CityId) -> Option<CityCardModel> {
    let session = game.session();
    let detail = session.city_view(city)?;
    let stat = |key: &str, value: String| Stat {
        label: l10n.text(key),
        value,
    };
    let peak = l10n.args(
        "city-peak-value",
        &[
            ("power", format::megawatts(detail.peak).into()),
            (
                "time",
                format::clock(u32::from(detail.peak_hour) * TICKS_PER_HOUR).into(),
            ),
        ],
    );
    let mut stats = vec![
        stat("city-base-load", format::megawatts(detail.base_load)),
        // An unconnected city's peak is what connecting it would add.
        stat(
            if detail.connected {
                "city-peak"
            } else {
                "city-would-add"
            },
            peak,
        ),
    ];
    // Set only in Operating, for a connected city, once a tick has run.
    if let (Some(demand), Some(served)) = (detail.demand_now, detail.served_now) {
        stats.push(stat("city-demand-now", format::megawatts(demand)));
        stats.push(stat("city-served-now", format::megawatts(served)));
        if let Some(cut) = cut_share(demand, served) {
            stats.push(stat("city-cut", format::percent(cut)));
        }
    }
    Some(CityCardModel {
        city,
        name: Names::new(l10n, session).city(city),
        connection: l10n.text(if detail.connected {
            "city-connected"
        } else {
            "city-not-connected"
        }),
        connected: detail.connected,
        stats,
        hint: (!detail.connected).then(|| l10n.text("city-connect-hint")),
    })
}

/// The share of `demand` not served, when any is cut: by a rotating outage, protection's shed or
/// both, which fall on every connected city alike.
fn cut_share(demand: Megawatts, served: Megawatts) -> Option<Ratio> {
    (demand > Megawatts::ZERO && served < demand)
        .then(|| Ratio::new((1.0 - served / demand).clamp(0.0, 1.0)).ok())
        .flatten()
}

/// The card: the name as a heading, the connection, then the figures and the hint. It has no
/// controls.
pub fn draw_city_card(ui: &mut egui::Ui, model: &CityCardModel) {
    ui.heading(model.name.as_str());
    ui.weak(model.connection.as_str());
    egui::Grid::new("city_card_stats")
        .num_columns(2)
        .spacing(egui::vec2(8.0, 1.0))
        .min_row_height(0.0)
        .show(ui, |ui| {
            for stat in &model.stats {
                ui.weak(stat.label.as_str());
                ui.label(stat.value.as_str());
                ui.end_row();
            }
        });
    if let Some(hint) = &model.hint {
        ui.weak(hint.as_str());
    }
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};
    use domain::region::PlantKind;
    use domain::session::{CityDetail, DayPhase};

    use super::*;
    use crate::hud::test_support::{eastern_game, empty_start, shipped_game, shipped_l10n};

    fn card(game: &Game, city: CityId) -> CityCardModel {
        city_card_model(game, &shipped_l10n(), city)
            .unwrap_or_else(|| panic!("a card for {city:?}"))
    }

    fn labels(card: &CityCardModel) -> Vec<&str> {
        card.stats.iter().map(|s| s.label.as_str()).collect()
    }

    fn value<'a>(card: &'a CityCardModel, label: &str) -> &'a str {
        &card
            .stats
            .iter()
            .find(|s| s.label == label)
            .unwrap_or_else(|| panic!("no {label:?} in {card:?}"))
            .value
    }

    fn detail(game: &Game, city: CityId) -> CityDetail {
        game.session().city_view(city).unwrap()
    }

    fn city(game: &Game, connected: bool) -> CityId {
        game.session()
            .city_views()
            .into_iter()
            .find(|c| c.connected == connected)
            .unwrap_or_else(|| panic!("a city with connected = {connected}"))
            .id
    }

    fn peak_value(detail: &CityDetail) -> String {
        format!(
            "{} at {}",
            format::megawatts(detail.peak),
            format::clock(u32::from(detail.peak_hour) * TICKS_PER_HOUR)
        )
    }

    #[test]
    fn the_card_names_the_city_and_says_whether_it_is_connected() {
        let l10n = shipped_l10n();
        for region in ["eastern_interconnection", "western_interconnection"] {
            let game = shipped_game(region, Box::new(InMemoryCommandLog::default()));
            let names = Names::new(&l10n, game.session());

            let on = city(&game, true);
            let model = card(&game, on);
            let d = detail(&game, on);
            assert_eq!(model.city, on);
            assert_eq!(model.name, names.city(on));
            assert_ne!(
                model.name,
                l10n.args("name-city-unknown", &[("id", on.0.to_string().into())])
            );
            assert_eq!(model.connection, "Connected");
            assert!(model.connected);
            assert_eq!(model.hint, None, "{region}: no hint for a connected city");
            // Planning: no live rows.
            assert_eq!(labels(&model), ["Base load", "Peak today"], "{region}");
            assert_eq!(value(&model, "Base load"), format::megawatts(d.base_load));
            assert_eq!(value(&model, "Peak today"), peak_value(&d));

            let off = city(&game, false);
            let model = card(&game, off);
            let d = detail(&game, off);
            assert_eq!(model.name, names.city(off));
            assert_eq!(model.connection, "Not connected");
            assert!(!model.connected);
            assert_eq!(
                model.hint.as_deref(),
                Some("Build a line to it from the build menu to connect it.")
            );
            assert_eq!(
                labels(&model),
                ["Base load", "Would add at peak"],
                "{region}"
            );
            assert_eq!(value(&model, "Would add at peak"), peak_value(&d));

            assert_eq!(city_card_model(&game, &l10n, CityId(9_999)), None);
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn in_operating_a_connected_city_shows_its_demand_and_served_now() {
        let l10n = shipped_l10n();
        // The inherited schedule covers the day: nothing is cut.
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 5 }).unwrap();
        let tick = game.session().grid_snapshot().last_tick.unwrap();
        assert_eq!(tick.served, tick.demand, "no outage or shed");

        let on = city(&game, true);
        let model = card(&game, on);
        let d = detail(&game, on);
        assert_eq!(
            labels(&model),
            ["Base load", "Peak today", "Demand now", "Served now"]
        );
        assert_eq!(
            value(&model, "Demand now"),
            format::megawatts(d.demand_now.unwrap())
        );
        assert_eq!(
            value(&model, "Served now"),
            format::megawatts(d.served_now.unwrap())
        );

        // An unconnected city has no live figures, only what it would add.
        let off = card(&game, city(&game, false));
        assert_eq!(labels(&off), ["Base load", "Would add at peak"]);
        assert!(off.hint.is_some());
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn in_operating_during_an_outage_the_card_shows_the_share_cut() {
        let l10n = shipped_l10n();
        // Coal alone is committed: the day starts in a rotating outage.
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        for plant in game.session().plant_views() {
            if plant.kind == PlantKind::Coal {
                game.execute(Command::StartPlant { plant: plant.id })
                    .unwrap();
            }
        }
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 5 }).unwrap();
        assert_eq!(game.session().phase(), DayPhase::Operating);
        let tick = game.session().grid_snapshot().last_tick.unwrap();
        assert!(tick.outage > Ratio::ZERO, "an outage: {tick:?}");

        let on = city(&game, true);
        let model = card(&game, on);
        let d = detail(&game, on);
        assert_eq!(
            labels(&model),
            ["Base load", "Peak today", "Demand now", "Served now", "Cut"]
        );
        assert_eq!(model.stats.len(), MOST_CITY_STATS);
        let (demand, served) = (d.demand_now.unwrap(), d.served_now.unwrap());
        assert!(served < demand);
        let cut = Ratio::new(1.0 - served.value() / demand.value()).unwrap();
        assert_eq!(value(&model, "Cut"), format::percent(cut));
        // The city's cut is the system's: the same share of every connected city.
        let system = Ratio::new(1.0 - tick.served.value() / tick.demand.value()).unwrap();
        assert_eq!(format::percent(cut), format::percent(system));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_share_cut_is_none_unless_load_is_cut() {
        let mw = Megawatts::new;
        assert_eq!(cut_share(mw(100.0), mw(100.0)), None);
        assert_eq!(cut_share(mw(0.0), mw(0.0)), None);
        assert_eq!(
            cut_share(mw(200.0), mw(150.0)),
            Some(Ratio::new(0.25).unwrap())
        );
        assert_eq!(cut_share(mw(200.0), mw(0.0)), Some(Ratio::ONE));
    }
}

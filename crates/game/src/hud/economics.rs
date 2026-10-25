//! Pure formatting of domain-resolved construction terms; no pricing or timing rules here.
use super::plant_panel::PlantStat;
use crate::l10n::{
    Localizer,
    format::{self, MoneyStyle},
};
use application::Game;
use domain::{
    ids::PlantId,
    region::{EconomicZoneId, RegionDefinition},
    session::LineQuote,
};

pub(super) fn zone_name(
    region: &RegionDefinition,
    zone: EconomicZoneId,
    l10n: &Localizer,
) -> String {
    if zone.0 == 0 {
        l10n.text("construction-regional-default")
    } else {
        l10n.text(
            &region
                .economic_zone(zone)
                .expect("validated economic zone")
                .name_key,
        )
    }
}
pub(super) fn zone_stat(
    region: &RegionDefinition,
    zone: EconomicZoneId,
    l10n: &Localizer,
) -> PlantStat {
    PlantStat::new(
        l10n,
        "construction-economic-zone",
        zone_name(region, zone, l10n),
    )
}
pub(super) fn owned_plant_stats(
    game: &Game,
    plant: PlantId,
    l10n: &Localizer,
    style: &MoneyStyle,
) -> Vec<PlantStat> {
    if !game.session().region().has_economic_zones() {
        return Vec::new();
    }
    let quote = game
        .session()
        .plant_construction(plant)
        .expect("known plant");
    vec![
        zone_stat(game.session().region(), quote.economic_zone, l10n),
        PlantStat::new(l10n, "build-capital", format::money(quote.capital, style)),
        PlantStat::new(
            l10n,
            "build-months",
            l10n.args("build-months-value", &[("months", quote.months.into())]),
        ),
    ]
}
pub(super) fn route_stats(
    region: &RegionDefinition,
    quote: &LineQuote,
    l10n: &Localizer,
    style: &MoneyStyle,
) -> Vec<PlantStat> {
    if !region.has_economic_zones() {
        return Vec::new();
    }
    quote
        .zones
        .iter()
        .map(|work| PlantStat {
            label: zone_name(region, work.zone, l10n),
            value: l10n.args(
                "construction-route-work",
                &[
                    ("cells", work.cells.into()),
                    ("capital", format::money(work.capital, style).into()),
                    ("numerator", work.work_numerator.to_string().into()),
                    ("denominator", work.work_denominator.to_string().into()),
                ],
            ),
            color: super::theme::ValueColor::Tone(super::theme::TextTone::Information),
        })
        .collect()
}

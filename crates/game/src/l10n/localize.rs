//! Typed values to player-facing text.
//!
//! Every enum is matched exhaustively, with no `_` arm: adding a variant to the domain or the
//! application layer breaks the build here until it has a translation. Display values (money,
//! power, shares, times) are formatted by [`format`] and arrive in the messages as text.

use application::projections::{Lesson, MilestoneKind, MilestoneProgress, RampWarning};
use application::{
    CommandError, CommandLogError, RegionBundleProblem, RegionLoadError, SidecarProblem,
};
use domain::error::DomainError;
use domain::events::DomainEvent;
use domain::generation::PlantStatus;
use domain::region::{PlantKind, RegionError, SiteError};
use domain::regulation::StrikeReason;
use domain::session::{DayPhase, GameOverReason, IncidentKind};
use domain::time::{Season, SimDuration, TICKS_PER_HOUR};

use super::{FluentArg, Localizer, Names, format};
use crate::state::PhaseState;

/// Text for a typed value.
pub trait Localize {
    fn localize(&self, l10n: &Localizer) -> String;
}

/// Text for a typed value that mentions plants, cities or lines by id: `names` turns the ids into
/// names. These types (`CommandError`, `DomainError`, `IncidentKind`) deliberately have no
/// [`Localize`] impl, so a toast can't show "Plant #7" by forgetting the names. Text with no
/// session passes [`Names::empty`] and says so.
pub trait LocalizeWith {
    fn localize_with(&self, l10n: &Localizer, names: &Names) -> String;
}

pub trait LessonMoneyText {
    fn localize_money(&self, l10n: &Localizer, style: &format::MoneyStyle) -> String;
}

/// Text for a domain event that deserves a toast. Most events are bookkeeping (`TickCompleted`,
/// ledger entries, ...) and return `None`; the `match` is still exhaustive, so a new event must
/// be classified, but only a toast-worthy one needs words.
pub trait Toast {
    fn toast(&self, l10n: &Localizer, names: &Names, style: &format::MoneyStyle) -> Option<String>;
}

/// A numeric id, shown as `#7` in the copy.
fn id(value: u32) -> FluentArg {
    FluentArg::Text(value.to_string())
}

/// The id of a plant, city or line. The ids are tuple structs over `u32`.
macro_rules! id_of {
    ($value:expr) => {
        id($value.0)
    };
}

impl LocalizeWith for CommandError {
    fn localize_with(&self, l10n: &Localizer, names: &Names) -> String {
        match self {
            Self::Domain(error) => error.localize_with(l10n, names),
            Self::Region(error) => error.localize(l10n),
            Self::Log(error) => error.localize(l10n),
        }
    }
}

impl Localize for CommandLogError {
    fn localize(&self, l10n: &Localizer) -> String {
        l10n.args(
            "error-command-log",
            &[("message", self.message.as_str().into())],
        )
    }
}

impl LocalizeWith for DomainError {
    fn localize_with(&self, l10n: &Localizer, names: &Names) -> String {
        match self {
            Self::LineCrossesAreas => l10n.text("error-line-crosses-areas"),
            Self::ActivationBlocked { .. } => l10n.text("error-activation-blocked"),
            Self::ActivationShortfall { .. } => l10n.text("error-activation-shortfall"),
            Self::ActivationPlanChanged { .. } => l10n.text("error-activation-changed"),
            Self::LinkWithinArea => l10n.text("error-link-within-area"),
            Self::HvdcUnavailable => l10n.text("error-hvdc-unavailable"),
            Self::UnknownLink { .. } => l10n.text("error-unknown-link"),
            Self::LinkNotUnderConstruction { .. } => l10n.text("error-link-not-building"),
            Self::LinkNotInService { .. } => l10n.text("error-link-not-in-service"),
            Self::InvalidLinkTarget => l10n.text("error-link-target"),
            Self::ConstructionOverflow => l10n.text("error-construction-overflow"),
            Self::InvalidConstructionCell { .. } => l10n.text("error-construction-cell"),
            Self::InvalidRatio { value: _ } => l10n.text("error-invalid-ratio"),
            Self::InvalidMonth { month } => l10n.args(
                "error-invalid-month",
                &[("month", month.to_string().into())],
            ),
            Self::PlantNotBattery { plant } => l10n.args(
                "error-plant-not-battery",
                &[("plant", names.plant(*plant).into())],
            ),
            Self::PlantNotDispatchable { plant } => l10n.args(
                "error-plant-not-dispatchable",
                &[("plant", names.plant(*plant).into())],
            ),
            Self::PlantNotOffline { plant } => l10n.args(
                "error-plant-not-offline",
                &[("plant", names.plant(*plant).into())],
            ),
            Self::PlantNotOnline { plant } => l10n.args(
                "error-plant-not-online",
                &[("plant", names.plant(*plant).into())],
            ),
            Self::PlantNotRunning { plant } => l10n.args(
                "error-plant-not-running",
                &[("plant", names.plant(*plant).into())],
            ),
            Self::NonFiniteMegawatts { plant } => l10n.args(
                "error-non-finite-megawatts",
                &[("plant", names.plant(*plant).into())],
            ),
            Self::BelowMinimumStableOutput { plant, min } => l10n.args(
                "error-below-minimum-stable-output",
                &[
                    ("plant", names.plant(*plant).into()),
                    ("min", format::megawatts(*min).into()),
                ],
            ),
            Self::AboveCapacity { plant, max } => l10n.args(
                "error-above-capacity",
                &[
                    ("plant", names.plant(*plant).into()),
                    ("max", format::megawatts(*max).into()),
                ],
            ),
            Self::CommitmentInfeasible {
                gap,
                shutdown,
                startup,
            } => l10n.args(
                "error-commitment-infeasible",
                &[
                    ("gap", format::duration(*gap).into()),
                    ("shutdown", format::duration(*shutdown).into()),
                    ("startup", format::duration(*startup).into()),
                ],
            ),
            Self::DuplicateNode { node } => {
                l10n.args("error-duplicate-node", &[("node", id_of!(node))])
            }
            Self::UnknownNode { node } => {
                l10n.args("error-unknown-node", &[("node", id_of!(node))])
            }
            Self::LineToSelf { node: _ } => l10n.text("error-line-to-self"),
            Self::DuplicateLine { from, to } => l10n.args(
                "error-duplicate-line",
                &[
                    ("from", names.node(*from).into()),
                    ("to", names.node(*to).into()),
                ],
            ),
            Self::LineNotNeeded { from, to } => l10n.args(
                "error-line-not-needed",
                &[
                    ("from", names.node(*from).into()),
                    ("to", names.node(*to).into()),
                ],
            ),
            Self::DuplicateLineId { line } => l10n.args(
                "error-duplicate-line-id",
                &[("line", names.line(*line).into())],
            ),
            Self::UnknownPlant { plant } => {
                l10n.args("error-unknown-plant", &[("plant", id_of!(plant))])
            }
            Self::UnknownLine { line } => {
                l10n.args("error-unknown-line", &[("line", names.line(*line).into())])
            }
            Self::LineNotUnderConstruction { line } => l10n.args(
                "error-line-not-under-construction",
                &[("line", names.line(*line).into())],
            ),
            Self::WrongPhase { actual } => l10n.args(
                "error-wrong-phase",
                &[("phase", actual.localize(l10n).into())],
            ),
            Self::GameOver { reason } => l10n.args(
                "error-game-over",
                &[("reason", reason.localize(l10n).into())],
            ),
            Self::InvalidSite {
                kind,
                cell: _,
                reason,
            } => l10n.args(
                "error-invalid-site",
                &[
                    ("kind", kind.localize(l10n).into()),
                    ("reason", reason.localize(l10n).into()),
                ],
            ),
            Self::PlantNotCommissioned { plant } => l10n.args(
                "error-plant-not-commissioned",
                &[("plant", names.plant(*plant).into())],
            ),
            Self::StartUpBeyondDay { remaining, startup } => l10n.args(
                "error-start-up-beyond-day",
                &[
                    ("remaining", format::duration(*remaining).into()),
                    ("startup", format::duration(*startup).into()),
                ],
            ),
        }
    }
}

impl Localize for SiteError {
    fn localize(&self, l10n: &Localizer) -> String {
        match self {
            Self::Occupied => l10n.text("site-occupied"),
            Self::OutOfBounds => l10n.text("site-out-of-bounds"),
            Self::NotLand => l10n.text("site-not-land"),
            Self::InsufficientCoolingWater { have, need } => l10n.args(
                "site-insufficient-cooling-water",
                &[
                    ("have", format::percent(*have).into()),
                    ("need", format::percent(*need).into()),
                ],
            ),
            Self::NoHydroSite => l10n.text("site-no-hydro-site"),
        }
    }
}

impl Localize for RegionLoadError {
    fn localize(&self, l10n: &Localizer) -> String {
        match self {
            Self::NotFound(region) => l10n.args(
                "error-region-not-found",
                &[("region", region.as_str().into())],
            ),
            Self::Io { region, message } => l10n.args(
                "error-region-io",
                &[
                    ("region", region.as_str().into()),
                    ("message", message.as_str().into()),
                ],
            ),
            Self::Parse { region, message } => l10n.args(
                "error-region-parse",
                &[
                    ("region", region.as_str().into()),
                    ("message", message.as_str().into()),
                ],
            ),
            Self::Invalid { region, error } => l10n.args(
                "error-region-invalid",
                &[
                    ("region", region.as_str().into()),
                    ("detail", error.localize(l10n).into()),
                ],
            ),
            Self::IdMismatch { requested, found } => l10n.args(
                "error-region-id-mismatch",
                &[
                    ("requested", requested.as_str().into()),
                    ("found", found.as_str().into()),
                ],
            ),
            Self::Sidecar {
                region,
                file,
                problem,
            } => l10n.args(
                "error-region-sidecar",
                &[
                    ("region", region.as_str().into()),
                    ("file", (&**file).into()),
                    ("detail", problem.localize(l10n).into()),
                ],
            ),
            Self::Bundle { region, problem } => l10n.args(
                "error-region-bundle",
                &[
                    ("region", region.as_str().into()),
                    ("detail", problem.localize(l10n).into()),
                ],
            ),
        }
    }
}

impl Localize for SidecarProblem {
    fn localize(&self, l10n: &Localizer) -> String {
        match self {
            Self::Missing => l10n.text("region-sidecar-missing"),
            Self::Unreferenced => l10n.text("region-sidecar-unreferenced"),
            Self::InvalidName => l10n.text("region-sidecar-invalid-name"),
            Self::Io(message) => {
                l10n.args("region-sidecar-io", &[("message", message.as_str().into())])
            }
            Self::Parse(message) => l10n.args(
                "region-sidecar-parse",
                &[("message", message.as_str().into())],
            ),
        }
    }
}

impl Localize for RegionBundleProblem {
    fn localize(&self, l10n: &Localizer) -> String {
        l10n.text(match self {
            Self::UnsupportedVersion => "region-bundle-unsupported-version",
            Self::BadHeader => "region-bundle-bad-header",
            Self::Truncated => "region-bundle-truncated",
            Self::MainName => "region-bundle-main-name",
            Self::Unsorted => "region-bundle-unsorted",
            Self::NoSidecars => "region-bundle-no-sidecars",
        })
    }
}

impl Localize for RegionError {
    fn localize(&self, l10n: &Localizer) -> String {
        match self {
            Self::InvalidEconomicZone { zone, field } => l10n.args(
                "region-error-economic-zone",
                &[
                    (
                        "zone",
                        zone.map_or_else(|| "0".into(), |id| id.0.to_string())
                            .into(),
                    ),
                    ("field", (*field).into()),
                ],
            ),
            Self::MissingField { field } => {
                l10n.args("region-error-missing-field", &[("field", (*field).into())])
            }
            Self::UnsupportedRegionSchema(schema) => l10n.args(
                "region-error-unsupported-schema",
                &[("schema", schema.to_string().into())],
            ),
            Self::GridSizeMismatch { expected, actual } => l10n.args(
                "region-error-grid-size-mismatch",
                &[
                    ("expected", expected.to_string().into()),
                    ("actual", actual.to_string().into()),
                ],
            ),
            Self::MissingPlantSpec(kind) => l10n.args(
                "region-error-missing-plant-spec",
                &[("kind", kind.localize(l10n).into())],
            ),
            Self::DuplicatePlantSpec(kind) => l10n.args(
                "region-error-duplicate-plant-spec",
                &[("kind", kind.localize(l10n).into())],
            ),
            Self::CityOutOfBounds(city) => {
                l10n.args("region-error-city-out-of-bounds", &[("city", id_of!(city))])
            }
            Self::CityNotOnLand(city) => {
                l10n.args("region-error-city-not-on-land", &[("city", id_of!(city))])
            }
            Self::CityCellOccupied { city, cell } => l10n.args(
                "region-error-city-cell-occupied",
                &[
                    ("city", id_of!(city)),
                    ("x", cell.x.to_string().into()),
                    ("y", cell.y.to_string().into()),
                ],
            ),
            Self::DuplicateCity(city) => {
                l10n.args("region-error-duplicate-city", &[("city", id_of!(city))])
            }
            Self::UnknownStartCity(city) => {
                l10n.args("region-error-unknown-start-city", &[("city", id_of!(city))])
            }
            Self::InvalidStartSite {
                kind,
                cell: _,
                reason,
            } => l10n.args(
                "region-error-invalid-start-site",
                &[
                    ("kind", kind.localize(l10n).into()),
                    ("reason", reason.localize(l10n).into()),
                ],
            ),
            Self::InvalidHomeCity(_) => l10n.text("region-error-invalid-home-city"),
            Self::InvalidShape { field } => {
                l10n.args("region-error-invalid-shape", &[("field", (*field).into())])
            }
            Self::InvalidBand { field } => {
                l10n.args("region-error-invalid-band", &[("field", (*field).into())])
            }
        }
    }
}

impl Localize for PlantKind {
    fn localize(&self, l10n: &Localizer) -> String {
        l10n.text(match self {
            Self::Nuclear => "plant-kind-nuclear",
            Self::Coal => "plant-kind-coal",
            Self::Ccgt => "plant-kind-ccgt",
            Self::Peaker => "plant-kind-peaker",
            Self::Solar => "plant-kind-solar",
            Self::Hydro => "plant-kind-hydro",
            Self::Battery => "plant-kind-battery",
            Self::Wind => "plant-kind-wind",
        })
    }
}

impl Localize for PlantStatus {
    fn localize(&self, l10n: &Localizer) -> String {
        match self {
            Self::UnderConstruction { months_left } => l10n.args(
                "plant-status-under-construction",
                &[("months", (*months_left).into())],
            ),
            Self::Offline => l10n.text("plant-status-offline"),
            Self::Starting { ticks_left } => l10n.args(
                "plant-status-starting",
                &[(
                    "time",
                    format::duration(SimDuration::from_ticks(*ticks_left)).into(),
                )],
            ),
            Self::Online { .. } => l10n.text("plant-status-online"),
            Self::Stopping { .. } => l10n.text("plant-status-stopping"),
        }
    }
}

impl Localize for DayPhase {
    fn localize(&self, l10n: &Localizer) -> String {
        l10n.text(match self {
            Self::Planning => "phase-planning",
            Self::Operating => "phase-operating",
            Self::Review => "phase-review",
        })
    }
}

impl Localize for PhaseState {
    fn localize(&self, l10n: &Localizer) -> String {
        l10n.text(match self {
            Self::Planning => "phase-planning",
            Self::Operating => "phase-operating",
            Self::Review => "phase-review",
        })
    }
}

impl Localize for Season {
    fn localize(&self, l10n: &Localizer) -> String {
        l10n.text(match self {
            Self::Winter => "season-winter",
            Self::Spring => "season-spring",
            Self::Summer => "season-summer",
            Self::Autumn => "season-autumn",
            Self::Monsoon => "season-monsoon",
            Self::PostMonsoon => "season-post-monsoon",
        })
    }
}

impl Localize for GameOverReason {
    fn localize(&self, l10n: &Localizer) -> String {
        l10n.text(match self {
            Self::Bankrupt => "game-over-bankrupt",
            Self::LicenseRevoked => "game-over-license-revoked",
        })
    }
}

impl Localize for StrikeReason {
    fn localize(&self, l10n: &Localizer) -> String {
        l10n.text(match self {
            Self::UnservedEnergy => "strike-reason-unserved-energy",
            Self::FrequencyPerformance => "strike-reason-frequency-performance",
        })
    }
}

impl Localize for MilestoneKind {
    fn localize(&self, l10n: &Localizer) -> String {
        match self {
            Self::ServePeak { gigawatts } => l10n.args(
                "milestone-serve-peak",
                &[("gigawatts", gigawatts.to_string().into())],
            ),
            Self::FullSeasonWithoutStrike => l10n.text("milestone-full-season-without-strike"),
            Self::ConnectCities { count } => l10n.args(
                "milestone-connect-cities",
                &[("count", count.to_string().into())],
            ),
        }
    }
}

impl Localize for MilestoneProgress {
    fn localize(&self, l10n: &Localizer) -> String {
        match self {
            Self::Peak { best, goal } => l10n.args(
                "milestone-progress-peak",
                &[
                    ("best", format::megawatts_like(*best, *goal).into()),
                    ("goal", format::megawatts(*goal).into()),
                ],
            ),
            Self::StrikeFreeMonths { months, needed } => l10n.args(
                "milestone-progress-strike-free",
                &[
                    ("months", months.to_string().into()),
                    ("needed", needed.to_string().into()),
                ],
            ),
            Self::Cities { connected, needed } => l10n.args(
                "milestone-progress-cities",
                &[
                    ("connected", connected.to_string().into()),
                    ("needed", needed.to_string().into()),
                ],
            ),
        }
    }
}

impl LocalizeWith for IncidentKind {
    fn localize_with(&self, l10n: &Localizer, names: &Names) -> String {
        match self {
            Self::LoadShed { mw } => l10n.args(
                "incident-load-shed",
                &[("mw", format::megawatts(*mw).into())],
            ),
            Self::Blackout => l10n.text("incident-blackout"),
            Self::BlackoutRestored => l10n.text("incident-blackout-restored"),
            Self::OverFrequencyTrip { plant } => l10n.args(
                "incident-over-frequency-trip",
                &[("plant", names.plant(*plant).into())],
            ),
            Self::RotatingOutage { share } => l10n.args(
                "incident-rotating-outage",
                &[("share", format::percent(*share).into())],
            ),
        }
    }
}

impl LessonMoneyText for Lesson {
    fn localize_money(&self, l10n: &Localizer, style: &format::MoneyStyle) -> String {
        match self {
            Self::ExpensivePeaking {
                energy,
                cost,
                alternative,
            } => l10n.args(
                "lesson-expensive-peaking",
                &[
                    ("energy", format::megawatt_hours(*energy).into()),
                    ("cost", format::money(*cost, style).into()),
                    ("alternative", format::price(*alternative, style).into()),
                ],
            ),
            Self::RotatingOutages { energy, peak_share } => l10n.args(
                "lesson-rotating-outages",
                &[
                    ("energy", format::megawatt_hours(*energy).into()),
                    ("peak_share", format::percent(*peak_share).into()),
                ],
            ),
            Self::UnservedEnergy {
                energy,
                first_incident_tick,
            } => l10n.args(
                "lesson-unserved-energy",
                &[
                    ("energy", format::megawatt_hours(*energy).into()),
                    ("time", format::clock(*first_incident_tick).into()),
                ],
            ),
            Self::Overgeneration { energy, trips: 0 } => l10n.args(
                "lesson-overgeneration",
                &[("energy", format::megawatt_hours(*energy).into())],
            ),
            Self::Overgeneration { energy, trips } => l10n.args(
                "lesson-overgeneration-trips",
                &[
                    ("energy", format::megawatt_hours(*energy).into()),
                    ("trips", (*trips).into()),
                ],
            ),
            Self::FrequencyPerformance {
                out_of_band,
                notice,
            } => l10n.args(
                if *notice {
                    "lesson-frequency-notice"
                } else {
                    "lesson-frequency-performance"
                },
                &[("out_of_band", format::percent(*out_of_band).into())],
            ),
            Self::RenewableCurtailed { energy } => l10n.args(
                "lesson-renewable-curtailed",
                &[("energy", format::megawatt_hours(*energy).into())],
            ),
            Self::SolarCurtailed { energy } => l10n.args(
                "lesson-solar-curtailed",
                &[("energy", format::megawatt_hours(*energy).into())],
            ),
            Self::StartUpCosts { total, share } => l10n.args(
                "lesson-start-up-costs",
                &[
                    ("total", format::money(*total, style).into()),
                    ("share", format::percent(*share).into()),
                ],
            ),
            Self::RateReview {
                tariff,
                allowed_return,
            } => l10n.args(
                "lesson-rate-review",
                &[
                    ("tariff", format::price(*tariff, style).into()),
                    ("allowed_return", format::percent(*allowed_return).into()),
                ],
            ),
            Self::AllClear => l10n.text("lesson-all-clear"),
        }
    }
}

impl Localize for RampWarning {
    fn localize(&self, l10n: &Localizer) -> String {
        l10n.args(
            "ramp-warning",
            &[
                ("change", format::megawatts(self.net_load_change).into()),
                ("minutes", self.horizon_minutes.to_string().into()),
                ("capability", format::megawatts(self.capability).into()),
            ],
        )
    }
}

impl Toast for DomainEvent {
    fn toast(&self, l10n: &Localizer, names: &Names, style: &format::MoneyStyle) -> Option<String> {
        let text = match self {
            Self::Blackout => l10n.text("event-blackout"),
            Self::BlackoutRestored => l10n.text("event-blackout-restored"),
            Self::RotatingOutageOrdered { hour, share } => {
                if share.value() > 0.0 {
                    l10n.args(
                        "event-rotating-outage-ordered",
                        &[
                            (
                                "time",
                                format::clock(u32::from(*hour) * TICKS_PER_HOUR).into(),
                            ),
                            ("share", format::percent(*share).into()),
                        ],
                    )
                } else {
                    l10n.text("event-rotating-outage-lifted")
                }
            }
            Self::StrikeIssued { strikes, reason } => l10n.args(
                "event-strike-issued",
                &[
                    ("strikes", strikes.to_string().into()),
                    ("reason", reason.localize(l10n).into()),
                ],
            ),
            Self::FrequencyNoticeIssued { month } => l10n.args(
                "event-frequency-notice",
                &[("month", format::month_year(*month).into())],
            ),
            Self::StrikeExpired { strikes } => l10n.args(
                "event-strike-expired",
                &[("strikes", u32::from(*strikes).into())],
            ),
            Self::LicenseRevoked => l10n.text("event-license-revoked"),
            Self::Bankrupt => l10n.text("event-bankrupt"),
            Self::PlantTripped { plant } => l10n.args(
                "event-plant-tripped",
                &[("plant", names.plant(*plant).into())],
            ),
            Self::OverFrequencyTrip { plant } => l10n.args(
                "event-over-frequency-trip",
                &[("plant", names.plant(*plant).into())],
            ),
            Self::HydroBudgetExhausted { plant } => l10n.args(
                "event-hydro-budget-exhausted",
                &[("plant", names.plant(*plant).into())],
            ),
            Self::CityConnected { city } => l10n.args(
                "event-city-connected",
                &[("city", names.city(*city).into())],
            ),
            Self::PlantCommissioned { plant, kind } => l10n.args(
                if kind.is_dispatchable() {
                    "event-plant-commissioned-agc"
                } else {
                    "event-plant-commissioned"
                },
                &[("plant", names.plant_as(*plant, *kind).into())],
            ),
            Self::LineCommissioned { line } => l10n.args(
                "event-line-commissioned",
                &[("line", names.line(*line).into())],
            ),
            Self::LineConstructionCancelled { line } => l10n.args(
                "event-line-construction-cancelled",
                &[("line", names.line(*line).into())],
            ),
            Self::RateReviewCompleted {
                tariff,
                allowed_return,
            } => l10n.args(
                "event-rate-review-completed",
                &[
                    ("tariff", format::price(*tariff, style).into()),
                    ("allowed_return", format::percent(*allowed_return).into()),
                ],
            ),
            // Bookkeeping, not news: no toast.
            Self::PlantStarting { .. }
            | Self::PlantOnline { .. }
            | Self::PlantStopped { .. }
            | Self::TickCompleted(_)
            | Self::FrequencyDeviated { .. }
            | Self::LoadShed { .. }
            | Self::SolarCurtailed { .. }
            | Self::RenewableCurtailed { .. }
            | Self::LedgerEntryRecorded(_)
            | Self::CapitalHeld { .. }
            | Self::CapitalReleased { .. }
            | Self::CapitalDisallowed { .. }
            | Self::PhaseChanged { .. }
            | Self::PlantConstructionStarted { .. }
            | Self::LineConstructionStarted { .. }
            | Self::BatteryDayClosed { .. }
            | Self::MonthClosed(_) => return None,
        };
        Some(text)
    }
}

impl Localize for domain::generation::BatteryMode {
    fn localize(&self, l10n: &Localizer) -> String {
        use domain::generation::BatteryMode::*;
        l10n.text(match self {
            Disabled => "battery-mode-disabled",
            Disconnected => "battery-mode-disconnected",
            Holding => "battery-mode-holding",
            Charging => "battery-mode-charging",
            Discharging => "battery-mode-discharging",
            Empty => "battery-mode-empty",
            Full => "battery-mode-full",
            GridSuspended => "battery-mode-suspended",
        })
    }
}

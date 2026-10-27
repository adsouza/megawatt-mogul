//! Planning analysis is immutable until a command changes the session or localisation changes.
use application::Game;
use bevy::prelude::*;
use domain::session::{CapacityShortfallWarning, DayPhase, GameSession};

use super::{ForecastModel, UiState};
use crate::{
    l10n::Localizer,
    sim::{Session, SessionGeneration},
};

#[derive(Debug, Clone)]
pub struct PlanningForecast {
    pub(super) activations:
        std::collections::BTreeMap<domain::ids::NodeId, super::ActivationCardModel>,
    pub(super) chart: ForecastModel,
    pub(super) shortfall: Option<CapacityShortfallWarning>,
    pub(super) warnings: super::planning_warnings::WarningsModel,
}

impl PlanningForecast {
    pub fn of(game: &Game, l10n: &Localizer) -> Self {
        let session = game.session();
        let supply = session.supply_forecast();
        Self {
            activations: session
                .power()
                .island_nodes()
                .keys()
                .filter(|id| !session.power().island_active(**id))
                .filter_map(|id| {
                    Some((*id, super::activation::activation_card(session, *id, l10n)?))
                })
                .collect(),
            warnings: super::planning_warnings::warnings_model(game, l10n, &UiState::default()),
            chart: super::forecast_chart::forecast_model_for(
                game,
                l10n,
                &UiState::default(),
                &supply,
            ),
            shortfall: if session.region().areas().is_some() {
                session.capacity_shortfall_check()
            } else {
                GameSession::capacity_shortfall_for(&supply)
            },
        }
    }
}

#[derive(Resource, Default)]
pub struct PlanningForecastCache {
    forecast: Option<PlanningForecast>,
    source: Option<(u32, u32, Option<u64>)>,
}

impl PlanningForecastCache {
    pub fn get(&self) -> Option<&PlanningForecast> {
        self.forecast.as_ref()
    }
}

pub(super) fn refresh(
    session: Option<Res<Session>>,
    l10n: Option<Res<Localizer>>,
    generation: Option<Res<SessionGeneration>>,
    mut cache: ResMut<PlanningForecastCache>,
) {
    let (Some(session), Some(l10n)) = (session, l10n) else {
        if cache.forecast.is_some() {
            *cache = PlanningForecastCache::default();
        }
        return;
    };
    if session.0.session().phase() != DayPhase::Planning {
        if cache.forecast.is_some() {
            *cache = PlanningForecastCache::default();
        }
        return;
    }
    let source = (
        session.last_changed().get(),
        l10n.last_changed().get(),
        generation.map(|g| g.0),
    );
    if cache.source != Some(source) {
        cache.forecast = Some(PlanningForecast::of(&session.0, &l10n));
        cache.source = Some(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};
    use application::{Command, InMemoryCommandLog};

    #[test]
    fn idle_frames_reuse_analysis_but_plan_edits_refresh_it() {
        let mut app = App::new();
        app.insert_resource(Session(eastern_game(Box::new(
            InMemoryCommandLog::default(),
        ))))
        .insert_resource(shipped_l10n())
        .init_resource::<PlanningForecastCache>()
        .add_systems(Update, refresh);
        app.update();
        let first = app
            .world()
            .resource_ref::<PlanningForecastCache>()
            .last_changed();
        let hash = app.world().resource::<Session>().0.session().state_hash();
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Session>().0.session().state_hash(),
            hash
        );
        assert_eq!(
            app.world()
                .resource_ref::<PlanningForecastCache>()
                .last_changed(),
            first
        );
        let ids: Vec<_> = app
            .world()
            .resource::<Session>()
            .0
            .session()
            .plant_views()
            .into_iter()
            .filter(|p| p.kind.is_dispatchable())
            .map(|p| p.id)
            .collect();
        for plant in ids {
            app.world_mut()
                .resource_mut::<Session>()
                .0
                .execute(Command::CommitPlant {
                    plant,
                    hours: [false; 24],
                })
                .unwrap();
        }
        app.update();
        let cache = app.world().resource_ref::<PlanningForecastCache>();
        assert_ne!(cache.last_changed(), first);
        assert!(cache.get().unwrap().shortfall.is_some());
        assert_ne!(
            app.world().resource::<Session>().0.session().state_hash(),
            hash
        );
    }

    #[test]
    fn cache_clears_outside_planning_and_refreshes_for_locale_and_replacement_sessions() {
        let mut app = App::new();
        app.insert_resource(Session(eastern_game(Box::new(
            InMemoryCommandLog::default(),
        ))))
        .insert_resource(shipped_l10n())
        .init_resource::<SessionGeneration>()
        .init_resource::<PlanningForecastCache>()
        .add_systems(Update, refresh);
        app.update();
        let changed = |app: &App| {
            app.world()
                .resource_ref::<PlanningForecastCache>()
                .last_changed()
        };
        let first = changed(&app);
        app.world_mut().resource_mut::<SessionGeneration>().0 += 1;
        app.update();
        assert_ne!(changed(&app), first);
        let previous = changed(&app);
        app.insert_resource(shipped_l10n());
        app.update();
        assert_ne!(changed(&app), previous);
        app.world_mut()
            .resource_mut::<Session>()
            .0
            .execute(Command::BeginOperating)
            .unwrap();
        app.update();
        assert!(
            app.world()
                .resource::<PlanningForecastCache>()
                .get()
                .is_none()
        );
        let cleared = changed(&app);
        app.update();
        assert_eq!(changed(&app), cleared);
        app.world_mut().remove_resource::<Session>();
        app.update();
        app.insert_resource(Session(eastern_game(Box::new(
            InMemoryCommandLog::default(),
        ))));
        app.update();
        let cache = app.world().resource::<PlanningForecastCache>();
        assert!(cache.get().is_some());
        assert_eq!(
            cache.get().unwrap().shortfall,
            app.world()
                .resource::<Session>()
                .0
                .session()
                .capacity_shortfall_check()
        );
    }
}

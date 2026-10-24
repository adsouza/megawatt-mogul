use super::{
    CameraPose, MapSurface, TuningConfig, ViewSync, WorldAssets, WorldControls, WorldScene,
};
use crate::{hud::HudSet, sim::SimSet};
use bevy::{
    camera::{CameraOutputMode, CameraUpdateSystems, Hdr, Viewport, visibility::RenderLayers},
    post_process::{bloom::Bloom, dof::DepthOfField},
    prelude::*,
    transform::TransformSystems,
};
use bevy_egui::{
    EguiGlobalSettings, EguiPostUpdateSet, EguiPrimaryContextPass, PrimaryEguiContext,
};

/// World entities/assets and the transparent HUD surface; also works without a renderer in tests.
pub struct WorldViewPlugin;
impl Plugin for WorldViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<WorldScene>()
            .init_resource::<WorldAssets>()
            .init_resource::<ViewSync>()
            .init_resource::<crate::hud::Selection>()
            .init_resource::<crate::hud::MapLayer>()
            .add_plugins(OverlayPlugin)
            .init_resource::<MapSurface>()
            .add_systems(Update, super::render::reconcile.after(SimSet::Apply))
            .add_systems(
                EguiPrimaryContextPass,
                super::ui::finish_surface
                    .after(HudSet::Overlays)
                    .run_if(resource_exists::<crate::l10n::Localizer>),
            );
    }
}
/// Phase treatment and visual animation, independent of the domain clock.
pub struct OverlayPlugin;
impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldControls>()
            .init_resource::<TuningConfig>()
            .init_resource::<super::flow::FlowAnimation>()
            .add_systems(
                PostUpdate,
                (
                    super::render::update_visuals,
                    super::lighting::update_sunlight,
                    super::flow::update_pulses,
                )
                    .chain()
                    .after(EguiPostUpdateSet::EndPass)
                    .before(TransformSystems::Propagate),
            );
    }
}
#[derive(Component)]
pub struct DioramaCamera;
/// Full-window egui camera and a separate clipped 3D camera; both live in the library.
pub struct CameraPlugin;
impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalAmbientLight {
            color: Color::srgb(1.0, 0.88, 0.72),
            brightness: 350.0,
            ..default()
        })
        .add_plugins(super::viewport_render::ViewportTargetPlugin)
        .add_systems(Startup, setup)
        .add_systems(
            PostUpdate,
            update_camera
                .after(EguiPostUpdateSet::EndPass)
                .before(CameraUpdateSystems)
                .before(TransformSystems::Propagate),
        );
    }
}
fn setup(mut commands: Commands, mut settings: ResMut<EguiGlobalSettings>) {
    settings.auto_create_primary_context = false;
    commands.spawn((
        Camera2d,
        PrimaryEguiContext,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            output_mode: CameraOutputMode::Write {
                blend_state: Some(bevy::render::render_resource::BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            ..default()
        },
        // egui supplies its own edge antialiasing. Multisampling this otherwise empty 2D
        // pass only allocates large color/depth buffers; Camera2d and empty render layers
        // also keep the HUD out of the 3D shadow pipeline.
        Msaa::Off,
        RenderLayers::none(),
    ));
    spawn_diorama_camera(&mut commands, None, None);
}

fn spawn_diorama_camera(
    commands: &mut Commands,
    viewport: Option<Viewport>,
    pose: Option<CameraPose>,
) -> Entity {
    commands
        .spawn((
            DioramaCamera,
            Camera3d::default(),
            Hdr,
            bevy::core_pipeline::tonemapping::Tonemapping::ReinhardLuminance,
            Camera {
                is_active: viewport.is_some() && pose.is_some(),
                viewport,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.14, 0.18, 0.23)),
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: super::camera::FOV,
                aspect_ratio: pose.map_or(1.0, |p| p.aspect),
                near: 0.1,
                far: 2000.0,
                ..default()
            }),
            pose.map_or_else(Transform::default, |p| p.transform()),
            Msaa::Off,
            bevy::light::ShadowFilteringMethod::Gaussian,
        ))
        .id()
}
fn update_camera(
    mut commands: Commands,
    surface: Res<MapSurface>,
    scene: Res<WorldScene>,
    controls: Res<WorldControls>,
    tuning: Res<TuningConfig>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut cameras: Query<(Entity, &mut Camera, &mut Transform, &mut Projection), With<DioramaCamera>>,
) {
    if scene
        .model
        .as_ref()
        .is_some_and(|m| m.phase == domain::session::DayPhase::Review)
    {
        for (entity, _, _, _) in &mut cameras {
            commands.entity(entity).despawn();
        }
        return;
    }
    let Some(window) = windows.iter().next() else {
        return;
    };
    let view = surface.rect.and_then(|rect| {
        super::physical_viewport(
            rect,
            surface.scale,
            UVec2::new(window.physical_width(), window.physical_height()),
        )
    });
    let active = scene.model.is_some() && view.is_some() && surface.pose.is_some();
    if cameras.is_empty() && active {
        let entity = spawn_diorama_camera(&mut commands, view.clone(), surface.pose);
        set_effects(
            &mut commands,
            entity,
            &controls,
            &tuning,
            &surface,
            window,
            view.as_ref(),
        );
    }
    for (entity, mut camera, mut transform, mut projection) in &mut cameras {
        camera.is_active = active;
        if !active {
            continue;
        }
        camera.viewport = view.clone();
        if let Some(pose) = surface.pose {
            *transform = pose.transform();
            if let Projection::Perspective(p) = &mut *projection {
                p.aspect_ratio = pose.aspect;
            }
        }
        set_effects(
            &mut commands,
            entity,
            &controls,
            &tuning,
            &surface,
            window,
            camera.viewport.as_ref(),
        );
    }
}

fn set_effects(
    commands: &mut Commands,
    entity: Entity,
    controls: &WorldControls,
    tuning: &TuningConfig,
    surface: &MapSurface,
    window: &Window,
    viewport: Option<&Viewport>,
) {
    if controls.effects {
        let distance = surface.pose.map_or(100.0, |p| p.eye.distance(p.target));
        // Compensate for the smaller target height to preserve blur radius in map pixels.
        let aperture = super::viewport_render::depth_of_field_aperture(
            viewport.map_or(window.physical_height(), |v| v.physical_size.y),
            window.physical_height(),
        );
        commands.entity(entity).insert((
            Bloom {
                intensity: tuning.bloom_intensity(),
                ..Bloom::NATURAL
            },
            DepthOfField {
                focal_distance: distance,
                aperture_f_stops: aperture,
                max_circle_of_confusion_diameter: 3.0,
                max_depth: 500.0,
                ..default()
            },
        ));
    } else {
        commands.entity(entity).remove::<(Bloom, DepthOfField)>();
    }
}

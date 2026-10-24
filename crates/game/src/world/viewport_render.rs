//! Keep the diorama's intermediate buffers local to its physical map viewport.

use super::DioramaCamera;
use bevy::{
    camera::{CameraOutputMode, Viewport},
    core_pipeline::{
        blit::{BlitPipeline, BlitPipelineKey},
        schedule::Core3d,
        upscaling::upscaling,
    },
    post_process::bloom::Bloom,
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        camera::ExtractedCamera,
        extract_component::ExtractComponent,
        render_resource::{
            BindGroup, CachedRenderPipelineId, PipelineCache, RenderPassDescriptor,
            SpecializedRenderPipelines, TextureViewId,
        },
        renderer::{RenderContext, ViewQuery},
        sync_world::RenderEntity,
        view::{ExtractedView, ResolvedCompositingSpace, ViewTarget},
    },
};

pub(super) struct ViewportTargetPlugin;

impl Plugin for ViewportTargetPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<CompositeBinding>()
            .add_systems(ExtractSchedule, extract_map_viewport)
            .add_systems(
                Render,
                (crop_targets, clear_inactive_binding)
                    .after(RenderSystems::ExtractCommands)
                    .before(RenderSystems::CreateViews),
            )
            .add_systems(Render, prepare_composite.in_set(RenderSystems::Prepare))
            .add_systems(Core3d, composite.after(upscaling));
    }
}

/// The output rectangle stays in window coordinates; only the intermediate view is rebased.
#[derive(Component)]
struct MapViewport {
    output: Viewport,
    bloom: Option<ExtractedBloom>,
}

type ExtractedBloom = <Bloom as ExtractComponent<RenderApp>>::Out;
type MapCameraComponents = (RenderEntity, &'static Camera, Option<&'static Bloom>);

#[derive(Component)]
struct CompositePipeline(CachedRenderPipelineId);

#[derive(Resource, Default)]
struct CompositeBinding(Option<(TextureViewId, BindGroup)>);

fn clear_inactive_binding(
    mut binding: ResMut<CompositeBinding>,
    views: Query<(), With<MapViewport>>,
) {
    if views.is_empty() {
        // A cached bind group owns its texture even after the view itself is removed.
        binding.0 = None;
    }
}

pub(super) fn depth_of_field_aperture(viewport_height: u32, window_height: u32) -> f32 {
    0.06 * (viewport_height as f32 / window_height.max(1) as f32)
}

fn local_bloom(bloom: &Bloom, camera: &Camera) -> Option<ExtractedBloom> {
    let mut settings = <Bloom as ExtractComponent<RenderApp>>::extract_component((bloom, camera))?;
    settings.1.viewport = Vec4::new(0.0, 0.0, 1.0, 1.0);
    Some(settings)
}

fn local_viewport(screen: &Viewport) -> Viewport {
    Viewport {
        physical_position: UVec2::ZERO,
        ..screen.clone()
    }
}

fn output_scissor(screen: &Viewport, output_size: UVec2) -> Option<URect> {
    let min = screen.physical_position.min(output_size);
    let max = screen
        .physical_position
        .saturating_add(screen.physical_size)
        .min(output_size);
    (max.x > min.x && max.y > min.y).then_some(URect { min, max })
}

fn extract_map_viewport(
    mut commands: Commands,
    cameras: Extract<Query<MapCameraComponents, With<DioramaCamera>>>,
) {
    for (entity, camera, bloom) in &cameras {
        if camera.is_active
            && let Some(viewport) = &camera.viewport
        {
            let bloom = bloom.and_then(|bloom| local_bloom(bloom, camera));
            commands.entity(entity).insert(MapViewport {
                output: viewport.clone(),
                bloom,
            });
        } else {
            commands.entity(entity).remove::<MapViewport>();
        }
    }
}

fn crop_targets(
    mut commands: Commands,
    mut cameras: Query<(
        Entity,
        &MapViewport,
        &mut ExtractedCamera,
        &mut ExtractedView,
    )>,
) {
    for (entity, screen, mut camera, mut view) in &mut cameras {
        let local = local_viewport(&screen.output);
        camera.physical_target_size = Some(local.physical_size);
        camera.viewport = Some(local.clone());
        // The stock full-window upscale is replaced by a one-to-one copy into the map rect.
        camera.output_mode = CameraOutputMode::Skip;
        view.viewport = UVec4::new(0, 0, local.physical_size.x, local.physical_size.y);
        // Bloom is extracted from the window-space camera before we rebase the render view.
        // Replace its normalized sampling rectangle after extraction, before uniform upload.
        if let Some(bloom) = &screen.bloom {
            commands.entity(entity).insert(bloom.clone());
        }
    }
}

fn prepare_composite(
    mut commands: Commands,
    mut cache: ResMut<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<BlitPipeline>>,
    blit: Res<BlitPipeline>,
    views: Query<(Entity, &ViewTarget, Option<&ResolvedCompositingSpace>), With<MapViewport>>,
) {
    for (entity, target, space) in &views {
        let Some(target_format) = target.out_texture_view_format() else {
            continue;
        };
        let pipeline = pipelines.specialize(
            &cache,
            &blit,
            BlitPipelineKey {
                target_format,
                blend_state: None,
                samples: 1,
                source_space: ResolvedCompositingSpace::space(space),
            },
        );
        cache.block_on_render_pipeline(pipeline);
        commands.entity(entity).insert(CompositePipeline(pipeline));
    }
}

fn composite(
    view: ViewQuery<(
        &ViewTarget,
        Option<&MapViewport>,
        Option<&CompositePipeline>,
    )>,
    cache: Res<PipelineCache>,
    blit: Res<BlitPipeline>,
    clear: Res<ClearColor>,
    mut binding: ResMut<CompositeBinding>,
    mut ctx: RenderContext,
) {
    let (target, Some(screen), Some(pipeline)) = view.into_inner() else {
        return;
    };
    let Some(pipeline) = cache.get_render_pipeline(pipeline.0) else {
        return;
    };
    let binding = &mut binding.0;
    let texture = target.main_texture_view();
    if binding.as_ref().is_none_or(|(id, _)| *id != texture.id()) {
        *binding = Some((
            texture.id(),
            blit.create_bind_group(ctx.render_device(), texture, &cache),
        ));
    }
    let Some((_, binding)) = binding.as_ref() else {
        return;
    };
    // A swapchain can shrink before the previous frame's view metadata catches up.
    // Clip the copy to the real attachment, keeping the original pixel-to-pixel viewport.
    let Some(output_texture) = target.out_texture() else {
        return;
    };
    let extent = output_texture.texture().size();
    let Some(scissor) = output_scissor(&screen.output, UVec2::new(extent.width, extent.height))
    else {
        return;
    };
    let Some(output) = target.out_texture_color_attachment(Some(clear.0.into())) else {
        return;
    };
    let mut pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("map_viewport_composite"),
            color_attachments: &[Some(output)],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    let position = screen.output.physical_position;
    let size = screen.output.physical_size;
    pass.set_viewport(
        position.x as f32,
        position.y as f32,
        size.x as f32,
        size.y as f32,
        0.0,
        1.0,
    );
    pass.set_scissor_rect(
        scissor.min.x,
        scissor.min.y,
        scissor.width(),
        scissor.height(),
    );
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, binding, &[]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
    use bevy::{ecs::schedule::ScheduleLabel, render::view::RetainedViewEntity};
    use proptest::prelude::*;

    #[test]
    fn shrinking_the_window_clips_a_stale_viewport_without_rescaling_it() {
        let screen = Viewport {
            physical_position: UVec2::new(260, 65),
            physical_size: UVec2::new(2512, 1423),
            ..default()
        };
        let scissor = output_scissor(&screen, UVec2::new(1280, 720)).unwrap();
        assert_eq!(scissor.min, UVec2::new(260, 65));
        assert_eq!(scissor.max, UVec2::new(1280, 720));
        assert_eq!(screen.physical_size, UVec2::new(2512, 1423));
        assert!(output_scissor(&screen, UVec2::new(100, 100)).is_none());
        assert!(output_scissor(&screen, UVec2::ZERO).is_none());
    }

    #[test]
    fn only_the_map_intermediates_shrink_while_projection_and_hud_stay_unchanged() {
        let screen = Viewport {
            physical_position: UVec2::new(260, 65),
            physical_size: UVec2::new(2512, 1423),
            ..default()
        };
        let projection = Mat4::from_scale(Vec3::new(0.8, 1.7, 0.1));
        let camera = || ExtractedCamera {
            target: None,
            physical_viewport_size: Some(screen.physical_size),
            physical_target_size: Some(UVec2::new(3072, 1728)),
            viewport: Some(screen.clone()),
            schedule: Core3d.intern(),
            order: 0,
            output_mode: default(),
            msaa_writeback: default(),
            clear_color: default(),
            sorted_camera_index_for_target: 0,
            exposure: 1.0,
            hdr: true,
        };
        let view = || ExtractedView {
            retained_view_entity: RetainedViewEntity::new(Entity::PLACEHOLDER.into(), None, 0),
            clip_from_view: projection,
            world_from_view: default(),
            clip_from_world: None,
            target_format: bevy::render::render_resource::TextureFormat::Rgba16Float,
            viewport: UVec4::new(260, 65, 2512, 1423),
            color_grading: default(),
            invert_culling: false,
        };
        let mut app = App::new();
        app.add_systems(Update, crop_targets);
        let map = app
            .world_mut()
            .spawn((
                camera(),
                view(),
                MapViewport {
                    output: screen.clone(),
                    bloom: None,
                },
            ))
            .id();
        let hud = app.world_mut().spawn((camera(), view())).id();
        let shadow = app.world_mut().spawn(view()).id();
        app.update();
        let map_camera = app.world().get::<ExtractedCamera>(map).unwrap();
        assert_eq!(map_camera.physical_target_size, Some(screen.physical_size));
        assert_eq!(
            map_camera.physical_viewport_size,
            Some(screen.physical_size)
        );
        assert_eq!(
            map_camera.viewport.as_ref().unwrap().physical_position,
            UVec2::ZERO
        );
        assert!(matches!(map_camera.output_mode, CameraOutputMode::Skip));
        let map_view = app.world().get::<ExtractedView>(map).unwrap();
        assert_eq!(map_view.viewport, UVec4::new(0, 0, 2512, 1423));
        assert_eq!(map_view.clip_from_view, projection);
        assert_eq!(
            app.world()
                .get::<ExtractedCamera>(hud)
                .unwrap()
                .physical_target_size,
            Some(UVec2::new(3072, 1728))
        );
        assert_eq!(
            app.world().get::<ExtractedView>(hud).unwrap().viewport,
            view().viewport
        );
        assert_eq!(
            app.world().get::<ExtractedView>(shadow).unwrap().viewport,
            view().viewport
        );
    }

    #[test]
    fn bloom_samples_the_local_target_instead_of_the_old_window_rectangle() {
        let camera = Camera {
            viewport: Some(Viewport {
                physical_position: UVec2::new(260, 65),
                physical_size: UVec2::new(2512, 1423),
                ..default()
            }),
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(3072, 1728),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        };
        let bloom = Bloom::NATURAL;
        let original =
            <Bloom as ExtractComponent<RenderApp>>::extract_component((&bloom, &camera)).unwrap();
        let local = local_bloom(&bloom, &camera).unwrap();
        assert_ne!(original.1.viewport, Vec4::new(0.0, 0.0, 1.0, 1.0));
        assert_eq!(local.1.viewport, Vec4::new(0.0, 0.0, 1.0, 1.0));
        assert_eq!(local.1.aspect, original.1.aspect);
        assert_eq!(local.1.scale, original.1.scale);
        assert_eq!(
            local.1.threshold_precomputations,
            original.1.threshold_precomputations
        );
        assert_eq!(local.0.intensity, bloom.intensity);
    }

    proptest! {
        #[test]
        fn the_output_scissor_always_fits_even_during_resize(
            x in any::<u32>(), y in any::<u32>(), width in any::<u32>(), height in any::<u32>(),
            output_width in 0u32..4000, output_height in 0u32..3000,
        ) {
            let screen = Viewport {
                physical_position: UVec2::new(x, y),
                physical_size: UVec2::new(width, height),
                ..default()
            };
            if let Some(rect) = output_scissor(&screen, UVec2::new(output_width, output_height)) {
                prop_assert!(rect.max.x <= output_width && rect.max.y <= output_height);
                prop_assert!(rect.width() > 0 && rect.height() > 0);
                prop_assert!(rect.min.x >= x && rect.min.y >= y);
            }
        }

        #[test]
        fn depth_of_field_keeps_the_same_blur_radius_in_physical_pixels(
            window_height in 100u32..4000, height_fraction in 1u32..=100,
        ) {
            let viewport_height = (window_height * height_fraction / 100).max(1);
            // All optical/depth factors are unchanged; the radius is proportional to H / N.
            let original_radius = window_height as f32 / 0.06;
            let local_radius = viewport_height as f32 / depth_of_field_aperture(viewport_height, window_height);
            prop_assert!((local_radius - original_radius).abs() <= original_radius * 1e-6);
        }

        #[test]
        fn viewport_targets_preserve_every_pixel_without_scaling(
            x in 0u32..2000, y in 0u32..1000, width in 1u32..3000, height in 1u32..2000,
            u in 0f32..1.0, v in 0f32..1.0,
        ) {
            let screen = Viewport {
                physical_position: UVec2::new(x, y),
                physical_size: UVec2::new(width, height),
                depth: 0.1..0.9,
            };
            let local = local_viewport(&screen);
            prop_assert_eq!(local.physical_position, UVec2::ZERO);
            prop_assert_eq!(local.physical_size, screen.physical_size);
            prop_assert_eq!(local.depth, screen.depth.clone());
            let pixel = Vec2::new(u, v) * local.physical_size.as_vec2();
            let composed = screen.physical_position.as_vec2() + pixel;
            let original = screen.physical_position.as_vec2()
                + Vec2::new(u, v) * screen.physical_size.as_vec2();
            prop_assert_eq!(composed, original);
        }
    }
}

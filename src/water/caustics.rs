//! Animated caustics projected by the main spot light.
//!
//! A fullscreen pass renders the caustic pattern (custom shader) into a
//! texture at the start of every rendered frame; the spot light uses it as a
//! light "cookie", so every lit surface — sand, rocks, plants, fish — receives
//! moving caustics, while the spot keeps casting real soft shadows.
//!
//! The pass is a plain system of the render graph, before the cameras: no
//! camera, no view, no 2D mesh to extract, batch and sort (an offscreen 2D
//! camera used to do this, for about 1 ms of CPU per frame).

use bevy::{
    core_pipeline::FullscreenShader,
    light::SpotLightTexture,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
            ColorTargetState, ColorWrites, FragmentState, LoadOp, Operations, PipelineCache,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, ShaderStages, ShaderType,
            StoreOp, TextureFormat, UniformBuffer, binding_types::uniform_buffer,
        },
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue},
        texture::GpuImage,
    },
};

use crate::{camera::OrbitCamera, environment::MainSpot, tank::WATER_Y};

const COOKIE_SIZE: u32 = 1024;

pub struct CausticsPlugin;

impl Plugin for CausticsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ExtractResourcePlugin::<Cookie>::default())
            .add_systems(Startup, spawn_cookie)
            .add_systems(PostUpdate, (attach_cookie, sync_cookie));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<CookieUniform>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(RenderGraph, draw.in_set(RenderGraphSystems::Begin));
    }
}

#[derive(ShaderType, Clone, Copy, Debug, Default)]
pub struct CookieParams {
    pub light_pos: Vec4,
    pub right: Vec4,
    pub up: Vec4,
    pub back: Vec4,
    /// x: shaft gain, y: caustic cells per metre, z: time scale, w: time (s).
    pub params: Vec4,
    pub drops: [Vec4; 4],
}

/// The cookie texture and what the pass needs to draw it this frame.
#[derive(Resource, Clone, ExtractResource)]
pub struct Cookie {
    image: Handle<Image>,
    pub params: CookieParams,
    /// The scene is rendered this frame (the still mode skips most frames).
    active: bool,
}

fn spawn_cookie(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new_target_texture(COOKIE_SIZE, COOKIE_SIZE, TextureFormat::Rgba8Unorm, None);
    // Written by the pass before anything samples it: no CPU copy.
    image.data = None;
    image.copy_on_resize = false;
    let image = images.add(image);
    commands.insert_resource(Cookie {
        image,
        params: CookieParams {
            params: vec4(0.7, 11.0, 1.0, 0.0),
            ..default()
        },
        active: false,
    });
}

fn attach_cookie(
    mut commands: Commands,
    cookie: Res<Cookie>,
    spots: Query<Entity, (With<MainSpot>, Without<SpotLightTexture>)>,
) {
    for spot in &spots {
        commands.entity(spot).insert(SpotLightTexture {
            image: cookie.image.clone(),
        });
    }
}

/// Keeps the pattern's projection in sync with the light's pose and cone, and
/// its clock with the simulation (the shaders' `globals.time`).
fn sync_cookie(
    time: Res<Time>,
    mut cookie: ResMut<Cookie>,
    spots: Query<(&GlobalTransform, &SpotLight), With<MainSpot>>,
    cams: Query<&Camera, With<OrbitCamera>>,
) {
    let Ok((transform, spot)) = spots.single() else {
        return;
    };
    let (_, rotation, translation) = transform.to_scale_rotation_translation();
    let c = &mut cookie.params;
    c.light_pos = translation.extend(spot.outer_angle.tan());
    c.right = (rotation * Vec3::X).extend(WATER_Y);
    // Light fraction that always gets through, and caustic gain.
    c.up = (rotation * Vec3::Y).extend(0.14);
    c.back = (rotation * Vec3::Z).extend(1.0);
    c.params.w = time.elapsed_secs_wrapped();
    cookie.active = cams.iter().any(|c| c.is_active);
}

#[derive(Resource)]
struct CookiePipeline {
    layout: BindGroupLayoutDescriptor,
    id: CachedRenderPipelineId,
}

#[derive(Resource, Default)]
struct CookieUniform {
    buffer: UniformBuffer<CookieParams>,
    bind_group: Option<BindGroup>,
}

fn init_pipeline(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    fullscreen: Res<FullscreenShader>,
    assets: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "caustic_cookie_layout",
        &BindGroupLayoutEntries::single(ShaderStages::FRAGMENT, uniform_buffer::<CookieParams>(false)),
    );
    let id = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("caustic_cookie".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: assets.load("shaders/caustic_cookie.wgsl"),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(CookiePipeline { layout, id });
}

fn prepare(
    cookie: Option<Res<Cookie>>,
    pipeline: Res<CookiePipeline>,
    pipeline_cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut uniform: ResMut<CookieUniform>,
) {
    let Some(cookie) = cookie.filter(|c| c.active) else {
        return;
    };
    let before = uniform.buffer.buffer().map(|b| b.id());
    uniform.buffer.set(cookie.params);
    uniform.buffer.write_buffer(&device, &queue);
    // The buffer is created on the first write only.
    if uniform.bind_group.is_none() || uniform.buffer.buffer().map(|b| b.id()) != before {
        let Some(binding) = uniform.buffer.binding() else {
            return;
        };
        uniform.bind_group = Some(device.create_bind_group(
            "caustic_cookie_bind_group",
            &pipeline_cache.get_bind_group_layout(&pipeline.layout),
            &BindGroupEntries::single(binding),
        ));
    }
}

fn draw(
    cookie: Option<Res<Cookie>>,
    pipeline: Res<CookiePipeline>,
    pipeline_cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    uniform: Res<CookieUniform>,
    mut ctx: RenderContext,
) {
    let Some(cookie) = cookie.filter(|c| c.active) else {
        return;
    };
    let (Some(target), Some(render_pipeline), Some(bind_group)) = (
        images.get(&cookie.image),
        pipeline_cache.get_render_pipeline(pipeline.id),
        uniform.bind_group.as_ref(),
    ) else {
        return;
    };
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("caustic_cookie"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &target.texture_view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(Default::default()),
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(render_pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

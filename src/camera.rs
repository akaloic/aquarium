//! Cinematic camera. By default the water fills the whole screen (wallpaper
//! framing) with a barely perceptible drift; the wheel zooms out to the whole
//! tank or dives into the water, right-drag orbits. Everything is damped.

use std::f32::consts::FRAC_PI_2;

use bevy::{
    anti_alias::{contrast_adaptive_sharpening::ContrastAdaptiveSharpening, taa::TemporalAntiAliasing},
    camera::{Exposure, Hdr, RenderTarget},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    light::{ShadowFilteringMethod, VolumetricFog},
    pbr::{ScreenSpaceTransmission, ScreenSpaceTransmissionQuality},
    post_process::{
        bloom::Bloom,
        dof::{DepthOfField, DepthOfFieldMode},
    },
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureFormat, TextureUsages},
        view::{ColorGrading, ColorGradingGlobal, ColorGradingSection},
    },
    window::{PrimaryWindow, WindowRef},
};

use crate::{
    config::{AppConfig, Quality},
    environment::{EnvironmentCubemap, camera_environment},
    quality::AdaptiveQuality,
    tank::{GLASS_T, HALF_D, HALF_W, WATER_Y},
};

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                ((orbit_input, orbit_update).chain(), freeze_environment_map, fit_scene_target),
            );
    }
}

const YAW_LIMIT: f32 = 1.05;
const PITCH_MIN: f32 = -0.06;
const PITCH_MAX: f32 = 0.85;

#[derive(Component)]
pub struct OrbitCamera {
    /// Smoothed state.
    yaw: f32,
    pitch: f32,
    zoom: f32,
    /// User offsets relative to the drone drift, relaxed back after inactivity.
    user_yaw: f32,
    user_pitch: f32,
    /// -1 = whole tank in the room, 0 = the water fills the screen, 1 = inside the water.
    goal_zoom: f32,
    idle: f32,
    /// Fixed preset (screenshots); disables drift and input.
    fixed: Option<(f32, f32, f32)>,
    drift_time: f32,
}

pub fn preset(name: &str) -> Option<(f32, f32, f32)> {
    // (yaw, pitch, zoom)
    Some(match name {
        "fill" => (0.0, 0.0, 0.0),
        "front" => (0.0, 0.07, -1.0),
        "hero" => (0.3, 0.1, -0.7),
        "side" => (0.75, 0.16, -0.9),
        "top" => (0.25, 0.8, -0.9),
        "low" => (-0.18, -0.05, -0.95),
        "close" => (0.2, 0.05, 0.45),
        "inside" => (0.12, 0.04, 0.93),
        _ => return None,
    })
}

/// Camera placement for a zoom level: (look-at target, distance, vertical FOV).
///
/// At zoom 0 the view is computed so that only the inside of the tank is
/// visible, edge to edge, whatever the screen aspect ratio: the frustum must fit
/// between the sand and the water line at the front glass, and inside the side
/// panes at the back glass. A narrow (telephoto) field of view keeps the
/// perspective gentle, like a photo of an aquarium.
fn rig(zoom: f32, aspect: f32) -> (Vec3, f32, f32) {
    let overview = (Vec3::new(0.0, 0.30, 0.0), 2.15, 38f32.to_radians());
    let inside = (Vec3::new(0.0, 0.36, 0.0), 0.16, 68f32.to_radians());
    let fill_dist = 1.91;
    let front = fill_dist - (HALF_D + GLASS_T);
    let back = fill_dist + HALF_D;
    let tan_v = ((WATER_Y * 0.5 - FILL_MARGIN.y) / front)
        .min((HALF_W - FILL_MARGIN.x) / (aspect.max(0.5) * back));
    let fill = (Vec3::new(0.0, WATER_Y * 0.5, 0.0), fill_dist, 2.0 * tan_v.atan());

    let ease = |t: f32| t * t * (3.0 - 2.0 * t);
    let mix = |a: (Vec3, f32, f32), b: (Vec3, f32, f32), t: f32| {
        (a.0.lerp(b.0, t), a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
    };
    if zoom <= 0.0 {
        mix(fill, overview, ease(-zoom.max(-1.0)))
    } else {
        mix(fill, inside, ease(zoom.min(1.0)))
    }
}

/// Room left around the frustum in "fill" framing, so the gentle drift never
/// reveals the outside of the tank (metres at the glass planes).
const FILL_MARGIN: Vec2 = Vec2::new(0.05, 0.022);

/// Amplitude of the automatic drift for a zoom level: tiny when the water fills
/// the screen, broad when the whole tank is visible.
fn drift_amplitude(zoom: f32) -> (f32, f32) {
    if zoom <= 0.0 {
        let t = (-zoom).min(1.0);
        (0.015 + 0.47 * t, 0.006 + 0.054 * t)
    } else {
        let t = zoom.min(1.0);
        (0.015 + 0.15 * t, 0.006 + 0.03 * t)
    }
}

/// Below the native resolution, the 3D scene is rendered offscreen, then shown
/// full-window through a UI image (bilinear upscale) by the presenter camera.
/// At 100 % it renders straight into the window: no intermediate image (22 MB
/// at 2940×1846), no full-screen copy, one camera less to prepare every frame.
#[derive(Resource)]
pub struct SceneTarget {
    pub image: Handle<Image>,
    /// Size of the 3D render (the window's in direct mode).
    pub size: UVec2,
    /// Rendering straight into the window.
    pub direct: bool,
    /// Megapixels of the render at full quality (level 0), for the cost model.
    pub full_mpx: f32,
}

/// The 2D camera that shows the offscreen image (and the UI) in the window.
#[derive(Component)]
pub struct Presenter;

/// The full-window UI image of the offscreen render.
#[derive(Component)]
struct SceneView;

/// Pixel cap of the 3D render for each quality preset: High renders at the
/// native resolution up to 4K UHD; the adaptive quality lowers it if needed.
fn pixel_budget(quality: Quality) -> f32 {
    match quality {
        Quality::High => 8.3e6,
        Quality::Medium => 4.4e6,
        Quality::Low => 2.0e6,
    }
}

fn new_scene_image(size: UVec2) -> Image {
    let mut image =
        Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    // Rendered into, never read on the CPU: no zero-filled copy to keep.
    image.data = None;
    image.copy_on_resize = false;
    image
}

fn spawn_camera(
    mut commands: Commands,
    config: Res<AppConfig>,
    env: Res<EnvironmentCubemap>,
    mut images: ResMut<Assets<Image>>,
) {
    let fixed = config.view.as_deref().and_then(preset);
    let steps = AdaptiveQuality::default().fog_steps(config.quality);
    let (yaw, pitch, zoom) = fixed.unwrap_or((0.0, 0.0, 0.0));
    let post = (
        Hdr,
        Msaa::Off,
        TemporalAntiAliasing::default(),
        Tonemapping::AcesFitted,
        DebandDither::Enabled,
        Bloom {
            intensity: 0.07,
            low_frequency_boost: 0.6,
            // A soft glow only: a small mip chain is enough and much cheaper.
            max_mip_dimension: 256,
            ..Bloom::NATURAL
        },
        Exposure { ev100: 7.0 },
        // Crisp details at native resolution, and when the adaptive quality
        // renders below it.
        ContrastAdaptiveSharpening {
            enabled: true,
            sharpening_strength: 0.35,
            denoise: false,
        },
        // Punchier grade: the water's in-scatter otherwise washes colours out.
        ColorGrading {
            global: ColorGradingGlobal {
                post_saturation: 1.12,
                ..default()
            },
            shadows: ColorGradingSection {
                saturation: 1.15,
                contrast: 1.12,
                ..default()
            },
            midtones: ColorGradingSection {
                saturation: 1.25,
                contrast: 1.1,
                ..default()
            },
            highlights: ColorGradingSection {
                saturation: 1.1,
                ..default()
            },
        },
    );
    let cam = commands.spawn((
        Name::new("camera"),
        Camera3d::default(),
        post,
        // Cheap 2x2 PCF: the volumetric fog samples the shadow map at every step.
        // Surfaces use PCSS (soft_shadows_enabled on the spot) anyway.
        ShadowFilteringMethod::Hardware2x2,
        VolumetricFog {
            // A faint blue fill so the water in the plants' shade isn't black.
            ambient_color: Color::srgb(0.25, 0.55, 1.0),
            ambient_intensity: 22.0,
            step_count: steps,
            // Fraction of a ray-march step (see volumetric_water.wgsl).
            jitter: 0.9,
            ..default()
        },
        ScreenSpaceTransmission {
            steps: 1,
            quality: ScreenSpaceTransmissionQuality::High,
        },
        Projection::Perspective(PerspectiveProjection {
            fov: 38f32.to_radians(),
            near: 0.02,
            far: 40.0,
            ..default()
        }),
        camera_environment(&env),
        OrbitCamera {
            yaw,
            pitch,
            zoom,
            user_yaw: 0.0,
            user_pitch: 0.0,
            goal_zoom: zoom,
            idle: 100.0,
            fixed,
            drift_time: 0.0,
        },
        transform_for(yaw, pitch, zoom, 1.6),
    )).id();
    // Capture mode: fixed offscreen size (AQ_RES=WxH to override).
    let size = std::env::var("AQ_RES")
        .ok()
        .and_then(|r| {
            let (w, h) = r.split_once('x')?;
            Some(UVec2::new(w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or(UVec2::new(1920, 1200));
    let image = images.add(new_scene_image(size));
    commands
        .entity(cam)
        .insert(RenderTarget::from(image.clone()));
    if config.screenshot.is_none() {
        // Presents the scene in the window, and hosts the UI (F3 overlay).
        commands.spawn((
            Name::new("presenter"),
            Camera2d,
            Camera {
                order: 1,
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            IsDefaultUiCamera,
            Presenter,
        ));
        commands.spawn((
            Name::new("scene view"),
            SceneView,
            ImageNode::new(image.clone()),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            ZIndex(-10),
        ));
    }
    commands.insert_resource(SceneTarget { image, size, direct: false, full_mpx: 0.0 });
    // Debug switches: AQ_DISABLE=taa,bloom,fog,env
    let off = std::env::var("AQ_DISABLE").unwrap_or_default();
    let mut e = commands.entity(cam);
    if config.dof {
        // Light depth of field; `interaction::autofocus` racks the focus onto
        // what the cursor points at, or the nearest fish. Opt-in: Bevy's DoF
        // works from the depth buffer and frays the edges of the volumetric
        // water and of the see-through layers.
        e.insert(DepthOfField {
            mode: DepthOfFieldMode::Gaussian,
            focal_distance: 1.95,
            sensor_height: 0.01866,
            aperture_f_stops: 2.8,
            max_circle_of_confusion_diameter: 12.0,
            max_depth: 10.0,
        });
    }
    if off.contains("taa") {
        e.remove::<TemporalAntiAliasing>();
    }
    if off.contains("bloom") {
        e.remove::<Bloom>();
    }
    if off.contains("dof") {
        e.remove::<DepthOfField>();
    }
    if off.contains("cas") {
        e.remove::<ContrastAdaptiveSharpening>();
    }
    if off.contains("fog") {
        e.remove::<VolumetricFog>();
    }
    if off.contains("sky") {
        e.remove::<bevy::light::Skybox>();
    }
    if off.contains("env") {
        e.remove::<(bevy::light::GeneratedEnvironmentMapLight, bevy::light::Skybox)>();
    }
}

fn transform_for(yaw: f32, pitch: f32, zoom: f32, aspect: f32) -> Transform {
    let (target, dist, _) = rig(zoom, aspect);
    let dir = vec3(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos());
    Transform::from_translation(target + dir * dist).looking_at(target, Vec3::Y)
}

fn orbit_input(
    config: Res<AppConfig>,
    mut cams: Query<&mut OrbitCamera>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
) {
    let Ok(mut cam) = cams.single_mut() else {
        return;
    };
    if cam.fixed.is_some() || config.wallpaper {
        return;
    }
    let dragging = buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Middle);
    if dragging && motion.delta != Vec2::ZERO {
        cam.user_yaw -= motion.delta.x * 0.0035;
        cam.user_pitch += motion.delta.y * 0.0025;
        cam.idle = 0.0;
    }
    if scroll.delta.y != 0.0 {
        let step = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y * 0.06,
            MouseScrollUnit::Pixel => scroll.delta.y * 0.0015,
        };
        cam.goal_zoom = (cam.goal_zoom + step).clamp(-1.0, 1.0);
        cam.idle = 0.0;
    }
}

fn orbit_update(
    time: Res<Time>,
    target: Res<SceneTarget>,
    mut cams: Query<(&mut OrbitCamera, &mut Transform, &mut Projection)>,
) {
    let Ok((mut cam, mut transform, mut projection)) = cams.single_mut() else {
        return;
    };
    let aspect = target.size.x as f32 / target.size.y.max(1) as f32;
    let dt = time.delta_secs().min(0.1);
    let (goal_yaw, goal_pitch, goal_zoom) = if let Some(f) = cam.fixed {
        f
    } else {
        cam.idle += dt;
        cam.drift_time += dt;
        let t = cam.drift_time;
        // After a while without input, relax the user offsets back to the drift,
        // and much later drift back to the full-screen framing.
        if cam.idle > 6.0 {
            let k = 1.0 - (-dt * 0.25).exp();
            cam.user_yaw *= 1.0 - k;
            cam.user_pitch *= 1.0 - k;
        }
        if cam.idle > 30.0 {
            cam.goal_zoom *= 1.0 - (1.0 - (-dt * 0.08).exp());
        }
        cam.user_yaw = cam.user_yaw.clamp(-2.0 * YAW_LIMIT, 2.0 * YAW_LIMIT);
        cam.user_pitch = cam.user_pitch.clamp(-1.0, 1.0);
        let zoom = cam.goal_zoom;
        // Drone drift: slow, layered, never repeating exactly. Its amplitude
        // depends on the framing so the "fill" view never shows the room.
        let (amp_yaw, amp_pitch) = drift_amplitude(zoom);
        let drift_yaw = amp_yaw * (0.82 * (t * 0.045).sin() + 0.18 * (t * 0.117 + 1.3).sin());
        let base_pitch = if zoom < 0.0 { 0.10 * (-zoom).min(1.0) } else { 0.03 * zoom.min(1.0) };
        let drift_pitch = base_pitch
            + amp_pitch * (0.75 * (t * 0.063 + 0.4).sin() + 0.25 * (t * 0.19).sin());
        (
            (drift_yaw + cam.user_yaw).clamp(-YAW_LIMIT, YAW_LIMIT),
            (drift_pitch + cam.user_pitch).clamp(PITCH_MIN, PITCH_MAX),
            zoom,
        )
    };

    // Critically damped-ish smoothing: no jerks, whatever the input.
    let smooth = |cur: f32, goal: f32, rate: f32| cur + (goal - cur) * (1.0 - (-dt * rate).exp());
    cam.yaw = smooth(cam.yaw, goal_yaw, 2.2);
    cam.pitch = smooth(cam.pitch, goal_pitch, 2.2);
    cam.zoom = smooth(cam.zoom, goal_zoom, 1.8);
    if cam.fixed.is_some() {
        (cam.yaw, cam.pitch, cam.zoom) = (goal_yaw, goal_pitch, goal_zoom);
    }

    let pitch = cam.pitch.clamp(PITCH_MIN, FRAC_PI_2 - 0.1);
    *transform = transform_for(cam.yaw, pitch, cam.zoom, aspect);
    if let Projection::Perspective(p) = projection.as_mut() {
        p.fov = rig(cam.zoom, aspect).2;
    }
    // Debug: `AQ_CAM=eye_x,eye_y,eye_z,look_x,look_y,look_z,fov_deg` pins the camera.
    if let Some(v) = std::env::var("AQ_CAM")
        .ok()
        .map(|s| s.split(',').filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>())
        .filter(|v| v.len() == 7)
    {
        *transform = Transform::from_xyz(v[0], v[1], v[2]).looking_at(Vec3::new(v[3], v[4], v[5]), Vec3::Y);
        if let Projection::Perspective(p) = projection.as_mut() {
            p.fov = v[6].to_radians();
        }
    }
}

/// `GeneratedEnvironmentMapLight` re-filters its cubemap every frame (meant for
/// dynamic probes). Our environment is static: keep the filtered result and
/// stop regenerating it after a few frames.
fn freeze_environment_map(
    mut commands: Commands,
    mut frames: Local<u32>,
    cams: Query<
        Entity,
        (
            With<bevy::light::GeneratedEnvironmentMapLight>,
            With<bevy::light::EnvironmentMapLight>,
        ),
    >,
) {
    for cam in &cams {
        *frames += 1;
        if *frames > 8 {
            commands
                .entity(cam)
                .remove::<bevy::light::GeneratedEnvironmentMapLight>();
        }
    }
}

/// Keeps the 3D render at `budget × adaptive` pixels, with the window's aspect
/// ratio: straight into the window at 100 %, offscreen below. A window size
/// change only counts once it has held for 0.4 s (a fullscreen window in the
/// background flickers between heights as the menu bar comes and goes): every
/// new size resets the temporal filters, which shows.
#[allow(clippy::too_many_arguments)]
fn fit_scene_target(
    mut commands: Commands,
    time: Res<Time<Real>>,
    config: Res<AppConfig>,
    adaptive: Res<AdaptiveQuality>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut target: ResMut<SceneTarget>,
    mut images: ResMut<Assets<Image>>,
    scene_cam: Query<Entity, With<OrbitCamera>>,
    presenter: Query<Entity, With<Presenter>>,
    mut view: Query<&mut Visibility, With<SceneView>>,
    mut seen: Local<(UVec2, f32, Option<u32>)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let win = UVec2::new(window.physical_width(), window.physical_height());
    if win.x < 16 || win.y < 16 {
        return;
    }
    let now = time.elapsed_secs();
    if win != seen.0 {
        seen.0 = win;
        seen.1 = now;
    }
    let level_changed = seen.2 != Some(adaptive.level);
    if !level_changed && now - seen.1 < 0.4 {
        return;
    }
    seen.2 = Some(adaptive.level);
    let phys = win.as_vec2();
    let budget = pixel_budget(config.quality);
    let full = (budget / (phys.x * phys.y)).sqrt().min(1.0);
    target.full_mpx = (phys * full).round().element_product() / 1e6;
    let scale = full * adaptive.render_scale();
    let size = (phys * scale).round().max(Vec2::splat(64.0)).as_uvec2();
    let direct = size == win && std::env::var("AQ_OFFSCREEN").is_err();
    if size == target.size && direct == target.direct {
        return;
    }
    let (Ok(cam), Ok(presenter)) = (scene_cam.single(), presenter.single()) else {
        return;
    };
    let Some(mut image) = images.get_mut(&target.image) else {
        return;
    };
    // Unused in direct mode: shrink it (memory) rather than drop it.
    let image_size = if direct { UVec2::splat(64) } else { size };
    if image.size() != image_size {
        image.resize(Extent3d {
            width: image_size.x,
            height: image_size.y,
            depth_or_array_layers: 1,
        });
    }
    if direct != target.direct {
        if direct {
            commands.entity(cam).insert((RenderTarget::Window(WindowRef::Primary), IsDefaultUiCamera));
            commands.entity(presenter).remove::<IsDefaultUiCamera>();
        } else {
            commands.entity(cam).insert(RenderTarget::from(target.image.clone())).remove::<IsDefaultUiCamera>();
            commands.entity(presenter).insert(IsDefaultUiCamera);
        }
        for mut v in &mut view {
            *v = if direct { Visibility::Hidden } else { Visibility::Inherited };
        }
    }
    target.size = size;
    target.direct = direct;
    info!(
        "scene render {}x{} ({:.0}% of the window, {})",
        size.x,
        size.y,
        scale * 100.0,
        if direct { "straight into the window" } else { "offscreen + upscale" }
    );
}

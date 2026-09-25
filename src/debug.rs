//! F3 performance overlay, adaptive quality, low-power frame cap and the
//! headless-ish screenshot mode used for visual tests.

use std::time::{Duration, Instant};

use bevy::{
    diagnostic::{
        DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
    },
    light::VolumetricFog,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};

use crate::config::AppConfig;

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin::default(),
        ))
        .add_systems(Startup, spawn_overlay)
        .add_systems(
            Update,
            (toggle_overlay, update_overlay, quit_on_escape, screenshot_mode),
        )
        .add_systems(Update, debug_toggles)
        .add_systems(PreUpdate, autopilot.after(bevy::input::InputSystems))
        .add_systems(Last, frame_limiter);
    }
}

#[derive(Component)]
struct Overlay;

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        Overlay,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::srgb(0.75, 0.95, 1.0)),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            padding: UiRect::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.02, 0.04, 0.6)),
        Visibility::Hidden,
    ));
}

fn toggle_overlay(keys: Res<ButtonInput<KeyCode>>, mut q: Query<&mut Visibility, With<Overlay>>) {
    if keys.just_pressed(KeyCode::F3) {
        for mut v in &mut q {
            *v = match *v {
                Visibility::Hidden => Visibility::Visible,
                _ => Visibility::Hidden,
            };
        }
    }
}

fn update_overlay(
    time: Res<Time>,
    mut timer: Local<f32>,
    diagnostics: Res<DiagnosticsStore>,
    config: Res<AppConfig>,
    adaptive: Res<crate::quality::AdaptiveQuality>,
    meshes: Query<&ViewVisibility, With<Mesh3d>>,
    fog: Query<&VolumetricFog>,
    windows: Query<&Window>,
    target: Option<Res<crate::camera::SceneTarget>>,
    power: Option<Res<crate::power::Power>>,
    mut text: Query<(&mut Text, &Visibility), With<Overlay>>,
) {
    *timer += time.delta_secs();
    if *timer < 0.25 {
        return;
    }
    *timer = 0.0;
    let Ok((mut text, vis)) = text.single_mut() else {
        return;
    };
    if *vis == Visibility::Hidden {
        return;
    }
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let entities = diagnostics
        .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        .and_then(|d| d.value())
        .unwrap_or(0.0);
    let total_meshes = meshes.iter().count();
    let visible = meshes.iter().filter(|v| v.get()).count();
    let steps = fog.iter().next().map(|f| f.step_count).unwrap_or(0);
    let res = windows
        .iter()
        .next()
        .map(|w| format!("{}x{}", w.physical_width(), w.physical_height()))
        .unwrap_or_default();
    let scene = target
        .map(|t| format!(" (3D {}x{})", t.size.x, t.size.y))
        .unwrap_or_default();
    let power_label = power
        .map(|p| format!("\npower      {:?} {:.0} fps x{:.2}{}", p.mode, p.target_fps, p.time_scale,
            if p.on_battery { format!("  battery {:.0}%", p.charge.unwrap_or(0.0) * 100.0) } else { String::new() }))
        .unwrap_or_default();
    text.0 = format!(
        "FPS        {fps:6.1}\n\
         frame      {frame_ms:6.2} ms\n\
         entities   {entities:6.0}\n\
         draws      {visible:4} / {total_meshes} meshes (visible)\n\
         god rays   {steps} samples\n\
         quality    {:?}  adaptive -{}\n\
         resolution {res}{scene}{}",
        config.quality,
        adaptive.level,
        power_label,
    );
}

fn quit_on_escape(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

/// `--low-power`: cap at 30 fps by sleeping at the end of the frame. As a
/// wallpaper hidden behind other windows, drop to 4 fps.
fn frame_limiter(
    power: Res<crate::power::Power>,
    quality: Res<crate::quality::AdaptiveQuality>,
    mut last: Local<Option<Instant>>,
) {
    // Uncapped while the start-up calibration measures frame times.
    let fps = if quality.calibrating() { f32::INFINITY } else { power.target_fps };
    // 60 fps is vsync's job; paused means the event loop is already idle.
    if !(fps > 0.0 && fps < 58.0) {
        *last = None;
        return;
    }
    let target = Duration::from_secs_f64(1.0 / fps as f64);
    if let Some(prev) = *last {
        let deadline = prev + target;
        // Sleep most of the remaining time, then spin: macOS oversleeps by ~1-3 ms.
        // At low rates precision doesn't matter: just sleep.
        let margin = if fps >= 20.0 { Duration::from_millis(3) } else { Duration::ZERO };
        let now = Instant::now();
        if deadline > now + margin {
            std::thread::sleep(deadline - now - margin);
        }
        while Instant::now() < deadline {
            std::hint::spin_loop();
        }
        // Keep a steady cadence even if a frame ran late.
        *last = Some(if Instant::now() > deadline + target { Instant::now() } else { deadline });
    } else {
        *last = Some(Instant::now());
    }
}

/// `--screenshot file.png`: render N frames, capture, exit.
fn screenshot_mode(
    mut commands: Commands,
    config: Res<AppConfig>,
    target: Option<Res<crate::camera::SceneTarget>>,
    mut frame: Local<u32>,
    mut timer: Local<Option<Instant>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = config.screenshot.clone() else {
        return;
    };
    *frame += 1;
    if *frame == config.frames.saturating_sub(60) {
        *timer = Some(Instant::now());
    }
    if *frame == config.frames {
        if let Some(t0) = *timer {
            let ms = t0.elapsed().as_secs_f64() * 1000.0 / 60.0;
            info!("capture: {ms:.2} ms/frame ({:.0} fps) over the last 60 frames", 1000.0 / ms);
        }
    }
    // `AQ_SEQ=from,count,every`: also save a sequence (`<file>_000.png`...).
    if let Some(v) = std::env::var("AQ_SEQ")
        .ok()
        .map(|s| s.split(',').filter_map(|x| x.parse::<u32>().ok()).collect::<Vec<_>>())
        .filter(|v| v.len() == 3 && v[2] > 0)
        && let Some(t) = &target
        && *frame >= v[0]
        && (*frame - v[0]) % v[2] == 0
        && (*frame - v[0]) / v[2] < v[1]
    {
        let k = (*frame - v[0]) / v[2];
        let stem = path.trim_end_matches(".png");
        commands.spawn(Screenshot::image(t.image.clone())).observe(save_to_disk(format!("{stem}_{k:03}.png")));
    }
    if *frame == config.frames {
        let shot = match target {
            Some(t) => Screenshot::image(t.image.clone()),
            None => Screenshot::primary_window(),
        };
        commands.spawn(shot).observe(save_to_disk(path.clone()));
    }
    if *frame > config.frames + 5 && std::path::Path::new(&path).exists() {
        exit.write(AppExit::Success);
    }
    if *frame > config.frames + 600 {
        error!("screenshot was not written, giving up");
        exit.write(AppExit::error());
    }
}

/// Profiling switches: `AQ_HIDE=name,..` hides entities whose name contains one
/// of the strings; `AQ_DISABLE=pcss|shadows` simplifies the main spot's shadows.
fn debug_toggles(
    mut frame: Local<u32>,
    mut named: Query<(&Name, &mut Visibility)>,
    mut spots: Query<&mut SpotLight>,
) {
    *frame += 1;
    if *frame > 120 {
        return;
    }
    let hide = std::env::var("AQ_HIDE").unwrap_or_default();
    if !hide.is_empty() {
        for (name, mut vis) in &mut named {
            if hide.split(',').any(|h| !h.is_empty() && name.as_str().contains(h)) {
                *vis = Visibility::Hidden;
            }
        }
    }
    let off = std::env::var("AQ_DISABLE").unwrap_or_default();
    for mut spot in &mut spots {
        if off.contains("pcss") {
            spot.soft_shadows_enabled = false;
        }
        if off.contains("shadows") {
            spot.shadow_maps_enabled = false;
        }
    }
}

/// `AQ_AUTOPILOT=1`: a virtual cursor sweeps the tank and clicks now and then
/// (headless tests of the interaction).
fn autopilot(
    mut frame: Local<u32>,
    target: Option<Res<crate::camera::SceneTarget>>,
    mut cursor: ResMut<crate::interaction::ExternalCursor>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
) {
    if std::env::var("AQ_AUTOPILOT").is_err() {
        return;
    }
    let Some(target) = target else {
        return;
    };
    *frame += 1;
    let t = *frame as f32 / 60.0;
    let size = target.size.as_vec2();
    // Slow sweep, then a quick swipe through the school every 6 s.
    let swipe = (t % 6.0 - 4.5).clamp(0.0, 0.6) / 0.6;
    let x = 0.5 + 0.3 * (t * 0.4).sin() + if swipe > 0.0 { (swipe - 0.5) * 0.6 } else { 0.0 };
    let y = 0.52 + 0.08 * (t * 0.7).sin();
    cursor.0 = Some(Vec2::new(x, y) * size);
    if *frame % 480 == 240 {
        buttons.press(MouseButton::Left);
    } else if buttons.pressed(MouseButton::Left) {
        buttons.release(MouseButton::Left);
    }
}

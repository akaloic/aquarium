//! Real-time photoreal aquarium — Bevy (Metal on macOS).

mod benthos;
mod camera;
mod config;
mod cputime;
mod debug;
mod environment;
mod fish;
mod interaction;
mod intro;
#[cfg(target_os = "macos")]
mod macos;
mod meshgen;
mod physics_probe;
mod power;
mod quality;
mod scape;
mod sdf;
mod tank;
mod textures;
mod wallpaper;
mod water;

use std::path::PathBuf;

use bevy::{
    light::PointLightShadowMap,
    prelude::*,
    window::{MonitorSelection, PresentMode, WindowMode, WindowResolution},
};

use config::AppConfig;

fn main() {
    let config = AppConfig::from_args();
    if let Some(path) = &config.screenshot {
        let _ = std::fs::remove_file(path);
    }

    let mut window = Window {
        title: "Aquarium".into(),
        // Uncapped in capture mode so the frame time report is meaningful.
        present_mode: if config.screenshot.is_some() {
            PresentMode::AutoNoVsync
        } else {
            PresentMode::AutoVsync
        },
        mode: if config.windowed || config.wallpaper {
            WindowMode::Windowed
        } else {
            WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
        },
        resolution: WindowResolution::new(1600, 1000),
        ..default()
    };
    if config.wallpaper {
        // Sized to the screen and sent to the desktop level by `wallpaper.rs`.
        window.decorations = false;
        window.resizable = false;
        window.window_level = bevy::window::WindowLevel::AlwaysOnBottom;
    }

    let headless = config.screenshot.is_some();
    let mut app = App::new();
    app.insert_resource(config)
        .insert_resource(ClearColor(Color::srgb(0.003, 0.004, 0.006)))
        .insert_resource(PointLightShadowMap {
            size: std::env::var("AQ_SHADOW").ok().and_then(|v| v.parse().ok()).unwrap_or(4096),
        })
        .add_plugins(
            DefaultPlugins
                .build()
                // Silent aquarium: an open audio stream would keep CoreAudio
                // (and the audio hardware) awake for nothing.
                .disable::<bevy::audio::AudioPlugin>()
                // No gamepads either (gilrs polls in two background threads).
                .disable::<bevy::gilrs::GilrsPlugin>()
                // A small scene: fewer worker threads means fewer wake-ups per
                // frame, which matters most when the wallpaper sleeps.
                .set(TaskPoolPlugin {
                    task_pool_options: TaskPoolOptions {
                        max_total_threads: std::env::var("AQ_THREADS")
                            .ok()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(6),
                        ..default()
                    },
                })
                .set(WindowPlugin {
                    // Capture mode renders offscreen without any window (no vsync cap).
                    primary_window: (!headless).then_some(window),
                    exit_condition: if headless {
                        bevy::window::ExitCondition::DontExit
                    } else {
                        bevy::window::ExitCondition::OnPrimaryClosed
                    },
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: assets_dir().to_string_lossy().into_owned(),
                    ..default()
                }),
        );
    if headless {
        app.add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
            std::time::Duration::ZERO,
        ));
    }
    app.add_plugins((
        // The scene.
        (
            textures::TexturesPlugin,
            environment::EnvironmentPlugin,
            tank::TankPlugin,
            water::WaterPlugin,
            scape::ScapePlugin,
            sdf::SdfPlugin,
            fish::FishPlugin,
            benthos::BenthosPlugin,
            interaction::InteractionPlugin,
            camera::CameraPlugin,
        ),
        // Running it: start-up, power, wallpaper, diagnostics.
        (
            intro::IntroPlugin,
            wallpaper::WallpaperPlugin,
            power::PowerPlugin,
            quality::QualityPlugin,
            debug::DebugPlugin,
            cputime::CpuTimePlugin,
            physics_probe::PhysicsProbePlugin,
        ),
    ))
    .run();
}

/// Finds the `assets` folder: next to the executable, in the working directory,
/// or at the crate root (for `cargo run` and `target/release/aquarium`).
pub fn assets_dir() -> PathBuf {
    let mut candidates = Vec::new();
    if let Ok(dir) = std::env::var("AQUARIUM_ASSETS") {
        candidates.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("assets"));
            candidates.push(dir.join("../../assets"));
            candidates.push(dir.join("../Resources/assets"));
        }
    }
    candidates.push(PathBuf::from("assets"));
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"));
    candidates
        .into_iter()
        .find(|p| p.join("shaders").is_dir())
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| {
            eprintln!("assets/ not found — run ./scripts/fetch_assets.sh from the repository root");
            PathBuf::from("assets")
        })
}

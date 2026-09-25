//! Energy management.
//!
//! | situation                                   | rendering        | simulation      |
//! |---------------------------------------------|------------------|-----------------|
//! | window hidden, screen locked, display asleep | none (paused)    | none (paused)   |
//! | mains power, awake                           | 60 fps (vsync)   | real time       |
//! | on battery, Low Power Mode, `--low-power`    | 30 fps           | real time       |
//! | wallpaper, nobody looking ("sleeping")       | 9 fps            | slow motion 20% |
//! | wallpaper, battery < 30 %, sleeping          | one frame / 2 s  | slow motion 5%  |
//!
//! The wallpaper wakes up (smooth ramp to full speed, for 30 s after the last
//! motion) when the mouse moves over the desktop or on ⌃⌥⌘A. While asleep the
//! process drops to the Darwin background priority band (efficiency cores).
//! When paused, winit only wakes every 2 s to check the screen state, the
//! cameras are off and the simulation clock is stopped: no draw, no boids.

use std::time::Duration;

use bevy::{
    prelude::*,
    winit::{UpdateMode, WinitSettings},
};

use crate::{
    camera::{Presenter, SceneTarget},
    config::AppConfig,
    interaction::ExternalCursor,
    wallpaper::Occluded,
};

pub struct PowerPlugin;

impl Plugin for PowerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Power>()
            .add_systems(
                PreUpdate,
                (crate::wallpaper::track_occlusion, poll_system, poll_desktop_cursor, decide).chain(),
            )
            .add_systems(Last, log_power);

    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Awake,
    /// Wallpaper nobody is looking at: slow motion, low frame rate.
    Sleeping,
    /// Sleeping on a low battery: an almost still picture.
    Still,
    /// Nothing visible: no rendering, no simulation.
    Paused,
}

#[derive(Resource)]
pub struct Power {
    pub mode: Mode,
    pub on_battery: bool,
    pub charge: Option<f32>,
    pub screen_off: bool,
    /// `NSProcessInfo.thermalState`: 0 nominal, 1 fair, 2 serious, 3 critical.
    pub thermal: u8,
    pub low_power_mode: bool,
    low_power: bool,
    /// 0 = asleep, 1 = fully awake (smoothed).
    pub awake: f32,
    /// Real time until which the wallpaper stays awake.
    wake_until: f64,
    /// Real time of the last sleep -> awake transition (the bottom dwellers startle).
    pub woke_at: f64,
    /// Frame rate the limiter aims at (0 = paused, infinite = uncapped).
    pub target_fps: f32,
    /// Simulation speed.
    pub time_scale: f32,
    /// Whether the 3D scene is rendered this frame (the still mode skips most).
    pub render: bool,
    next_battery_poll: f64,
    next_screen_poll: f64,
    next_desktop_check: f64,
    last_cursor: Option<(f64, f64)>,
    last_render: f64,
    background_priority: bool,
}

impl Default for Power {
    fn default() -> Self {
        Self {
            mode: Mode::Awake,
            on_battery: false,
            charge: None,
            screen_off: false,
            thermal: 0,
            low_power_mode: false,
            low_power: false,
            awake: 1.0,
            wake_until: 30.0,
            woke_at: -100.0,
            target_fps: 60.0,
            time_scale: 1.0,
            render: true,
            next_battery_poll: 0.0,
            next_screen_poll: 0.0,
            next_desktop_check: 0.0,
            last_cursor: None,
            last_render: -100.0,
            background_priority: false,
        }
    }
}

impl Power {
    /// Frame rate when awake: 30 on battery, in Low Power Mode, when the Mac is
    /// critically hot or with `--low-power`; 60 otherwise.
    pub fn awake_fps(&self) -> f32 {
        if self.low_power || self.on_battery || self.low_power_mode || self.thermal >= 3 { 30.0 } else { 60.0 }
    }

    /// Low battery: below 30 % and discharging.
    pub fn low_battery(&self) -> bool {
        self.on_battery && self.charge.is_some_and(|c| c < 0.3)
    }
    /// Wakes the wallpaper up for 30 s.
    pub fn wake(&mut self, now: f64) {
        self.wake_until = self.wake_until.max(now + 30.0);
    }
}

/// Battery every 10 s, screen lock / display sleep every 2 s (both cheap).
fn poll_system(config: Res<AppConfig>, time: Res<Time<Real>>, mut power: ResMut<Power>) {
    if config.screenshot.is_some() {
        return;
    }
    let now = time.elapsed_secs_f64();
    #[cfg(target_os = "macos")]
    {
        if now >= power.next_battery_poll {
            power.next_battery_poll = now + 10.0;
            let (mut on_battery, mut charge) = crate::macos::battery();
            // `AQ_BATTERY=1`: behave as on battery, 90 % (measurements).
            if std::env::var("AQ_BATTERY").is_ok() {
                (on_battery, charge) = (true, Some(0.9));
            }
            if on_battery != power.on_battery {
                info!("power: {}", if on_battery { "on battery" } else { "on mains" });
            }
            power.on_battery = on_battery;
            power.charge = charge;
        }
        if now >= power.next_screen_poll {
            power.next_screen_poll = now + 2.0;
            power.screen_off = crate::macos::screen_unavailable();
            let (thermal, lpm) = (crate::macos::thermal_state(), crate::macos::low_power_mode());
            if thermal != power.thermal || lpm != power.low_power_mode {
                info!("power: thermal state {thermal}, Low Power Mode {lpm}");
            }
            power.thermal = thermal;
            power.low_power_mode = lpm;
        }
        if crate::macos::HOTKEY_PRESSED.swap(false, std::sync::atomic::Ordering::Relaxed) {
            power.wake(now);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = now;
}

/// Wallpaper: the window ignores the mouse, so the cursor is read from the
/// system; it counts only over the visible desktop (not over other windows).
fn poll_desktop_cursor(
    config: Res<AppConfig>,
    time: Res<Time<Real>>,
    mut power: ResMut<Power>,
    mut external: ResMut<ExternalCursor>,
) {
    if !config.wallpaper || power.mode == Mode::Paused {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let now = time.elapsed_secs_f64();
        let Some(p) = crate::macos::cursor() else {
            return;
        };
        let moved = power.last_cursor.is_none_or(|q| (p.0 - q.0).abs() + (p.1 - q.1).abs() > 1.5);
        power.last_cursor = Some(p);
        if !moved {
            // Keep the last verdict: a still cursor over the desktop stays there.
            return;
        }
        // Window-list queries are cheap but not free: at most ~8 per second.
        if now < power.next_desktop_check {
            return;
        }
        power.next_desktop_check = now + 0.12;
        if crate::macos::over_desktop(p.0, p.1) {
            power.wake(now);
            // Window logical pixels: global points from the wallpaper's screen corner.
            let (x, y, _, _) = crate::macos::wallpaper_bounds().unwrap_or_default();
            external.0 = Some(Vec2::new((p.0 - x) as f32, (p.1 - y) as f32));
        } else {
            external.0 = None;
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (&time, &mut power, &mut external);
}

fn decide(
    config: Res<AppConfig>,
    time: Res<Time<Real>>,
    occluded: Res<Occluded>,
    target: Option<Res<SceneTarget>>,
    mut power: ResMut<Power>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut winit: ResMut<WinitSettings>,
    mut cameras: Query<(&mut Camera, Has<Presenter>)>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs().min(0.25);
    // `AQ_POWER_TEST=sleep|still` exercises the sleeping modes headless.
    let test = std::env::var("AQ_POWER_TEST").ok();
    if config.screenshot.is_some() && test.is_none() {
        // Capture / test runs: always awake and uncapped.
        power.target_fps = f32::INFINITY;
        return;
    }
    if let Some(t) = &test {
        power.wake_until = 3.0;
        power.on_battery = t == "still";
        power.charge = Some(if t == "still" { 0.2 } else { 1.0 });
    }

    let paused = occluded.0 || power.screen_off;
    power.low_power = config.low_power;
    let can_sleep = (config.wallpaper && !config.no_sleep) || test.is_some();
    let awake_goal = if !can_sleep || now < power.wake_until { 1.0 } else { 0.0 };
    // Wake up fast (1.2 s), fall asleep slowly (5 s): a cinematic slow-down.
    let rate = if awake_goal > power.awake { 1.0 / 1.2 } else { 1.0 / 5.0 };
    let was_asleep = power.awake < 0.5;
    power.awake = if awake_goal > power.awake {
        (power.awake + dt * rate).min(1.0)
    } else {
        (power.awake - dt * rate).max(0.0)
    };
    if was_asleep && power.awake >= 0.5 {
        power.woke_at = now;
    }
    let ease = power.awake * power.awake * (3.0 - 2.0 * power.awake);
    let awake_fps = power.awake_fps();
    let low = power.low_battery();

    let mode = if paused {
        Mode::Paused
    } else if ease > 0.001 {
        Mode::Awake
    } else if low {
        Mode::Still
    } else {
        Mode::Sleeping
    };
    // Asleep the scene runs at 9 fps; in the still mode the loop ticks at
    // 10 Hz (to notice the mouse) but the scene renders every 2 s only.
    let sleep_fps: f32 = 9.0;
    let sleep_speed: f32 = if low { 0.05 } else { 0.2 };
    power.target_fps = match mode {
        Mode::Paused => 0.0,
        Mode::Still => 10.0,
        _ => sleep_fps + (awake_fps - sleep_fps) * ease,
    };
    power.time_scale = if mode == Mode::Paused { 0.0 } else { sleep_speed + (1.0 - sleep_speed) * ease };
    power.render = match mode {
        Mode::Paused => false,
        Mode::Still => {
            let due = now - power.last_render >= 2.0;
            if due {
                power.last_render = now;
            }
            due
        }
        _ => true,
    };
    if mode != power.mode {
        info!(
            "power: {:?} -> {:?}{}{}",
            power.mode,
            mode,
            if occluded.0 { " (window hidden)" } else { "" },
            if power.screen_off { " (screen locked or asleep)" } else { "" }
        );
        power.mode = mode;
    }

    // Simulation clock.
    if mode == Mode::Paused {
        if !virtual_time.is_paused() {
            virtual_time.pause();
        }
    } else {
        if virtual_time.is_paused() {
            virtual_time.unpause();
        }
        if (virtual_time.relative_speed() - power.time_scale).abs() > 1e-3 {
            virtual_time.set_relative_speed(power.time_scale);
        }
    }

    // Event loop: while paused only wake up every 2 s (or on window events).
    let update = if mode == Mode::Paused {
        UpdateMode::reactive_low_power(Duration::from_secs(2))
    } else {
        UpdateMode::Continuous
    };
    if winit.focused_mode != update {
        winit.focused_mode = update;
        winit.unfocused_mode = update;
    }

    // Cameras: the 3D scene (and the caustics) only when rendering; the
    // presenter keeps showing the last image unless paused. In direct mode a
    // window no camera draws into keeps its last image by itself.
    let direct = target.is_some_and(|t| t.direct);
    for (mut cam, is_presenter) in &mut cameras {
        let active = if is_presenter { mode != Mode::Paused && !direct } else { power.render };
        if cam.is_active != active {
            cam.is_active = active;
        }
    }

    // Efficiency cores while nobody is looking.
    #[cfg(target_os = "macos")]
    {
        let background = config.wallpaper && mode != Mode::Awake;
        if background != power.background_priority {
            power.background_priority = background;
            crate::macos::set_background_priority(background);
        }
    }
}

/// `AQ_LOG_POWER=1`: logs the mode and the measured frame rate every 5 s.
fn log_power(
    time: Res<Time<Real>>,
    power: Res<Power>,
    mut acc: Local<(f64, u32, u32)>,
) {
    if std::env::var("AQ_LOG_POWER").is_err() {
        return;
    }
    acc.0 += time.delta_secs_f64();
    acc.1 += 1;
    acc.2 += power.render as u32;
    if acc.0 >= 5.0 {
        info!(
            "power log: {:?} awake {:.2} | {:.1} updates/s, {:.1} scene renders/s | target {:.0} fps, speed x{:.2} | battery {} {:?}",
            power.mode,
            power.awake,
            acc.1 as f64 / acc.0,
            acc.2 as f64 / acc.0,
            power.target_fps,
            power.time_scale,
            power.on_battery,
            power.charge
        );
        *acc = (0.0, 0, 0);
    }
}

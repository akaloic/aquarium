//! Dynamic resolution that never shows.
//!
//! Changing the render resolution resets the temporal filters (a burst of
//! grain in the water) and the sharpness jumps: done while someone watches,
//! it looks like the screen "refreshing". So the resolution only moves when
//! nobody can see it, and never by trial and error:
//!
//! - start-up, behind the black curtain: one short uncapped run at full
//!   resolution measures what a frame costs on this machine, which scales the
//!   scene's reference cost model (`fixed + per_mpx × megapixels`); the
//!   sharpest level that fits the frame budget is picked before the first
//!   image appears. Only a frame slower than the refresh can be timed: a
//!   faster one is held back by macOS (with or without vsync) and reads
//!   ~16.7 ms whatever its cost;
//! - running: under vsync the GPU lowers its clock to fill the frame, so the
//!   load macOS reports (IOKit) sits around 80 % at any level that fits and
//!   says nothing about headroom. Only frames missing the refresh for 4 s
//!   with the GPU saturated (the machine heating up, another app drawing)
//!   raise the cost model; otherwise it returns to the start-up measurement.
//!   A level that fits better is applied while the wallpaper sleeps or the
//!   window is hidden;
//! - frames missing the refresh for 4 s with the GPU saturated: the smallest
//!   step that ends it, not retried until the next launch (or, if macOS was
//!   reporting heat, until it has been cool for 5 min);
//! - the thermal state (fair, serious, critical) shrinks the budget before
//!   macOS throttles the GPU. At 30 fps (battery, Low Power Mode) the budget
//!   stays that of 60 fps: the time saved is energy saved (see `budget`).

use bevy::{
    light::VolumetricFog,
    prelude::*,
    window::{PresentMode, PrimaryWindow},
};

use crate::{
    camera::SceneTarget,
    config::{AppConfig, Quality},
    power::{Mode, Power},
    wallpaper::Occluded,
};

pub struct QualityPlugin;

impl Plugin for QualityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AdaptiveQuality>().add_systems(Update, (calibrate, adapt).chain());
    }
}

/// Render scale and fog samples (quarters of the preset) per level. The fog
/// cap rarely binds (a ray crosses 4–6 steps of water), so every level is a
/// resolution step.
const LADDER: [(f32, u32); 6] = [(1.0, 4), (0.88, 4), (0.77, 4), (0.67, 4), (0.58, 4), (0.5, 2)];

/// Reference cost of a frame of this scene (ms): `FIXED_MS + PER_MPX_MS ×
/// megapixels`, measured uncapped on an Apple M4 (4.2–5.9 ms + 3.4–3.8 ms/Mpx
/// over five runs).
const FIXED_MS: f32 = 5.0;
const PER_MPX_MS: f32 = 3.6;

/// Calibration frames per third.
const CHUNK: usize = 12;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Calibration {
    /// Waiting for the shaders and the decor (the curtain is down).
    Waiting,
    /// Uncapped frames at full resolution: frames to skip, then samples.
    Measuring { skip: u32 },
    Done,
}

#[derive(Resource)]
pub struct AdaptiveQuality {
    pub level: u32,
    pub calibration: Calibration,
    /// This machine against the reference model, as measured at start-up...
    calibrated_heat: f32,
    /// ...what the level may count on for now (raised by a forced drop)...
    base_heat: f32,
    /// ...and now (heat, other GPU users).
    heat: f32,
    /// Megapixels rendered at level 0.
    full_mpx: f32,
    samples: Vec<f32>,
    started: f32,
    /// Level to apply as soon as nobody is looking.
    pending: Option<u32>,
    frames: u32,
    late: u32,
    window: f32,
    late_windows: u32,
    headroom: f32,
    /// Seconds of headroom needed before a better level is scheduled (doubles
    /// each time one had to be undone).
    patience: f32,
    raised_at: f32,
    /// A drop was forced while macOS reported heat; since when it's cool.
    heat_drop: bool,
    cool_since: Option<f32>,
}

impl Default for AdaptiveQuality {
    fn default() -> Self {
        Self {
            level: 0,
            calibration: Calibration::Waiting,
            calibrated_heat: 1.0,
            base_heat: 1.0,
            heat: 1.0,
            // 2940×1846 until measured.
            full_mpx: 5.4,
            samples: Vec::new(),
            started: 0.0,
            pending: None,
            frames: 0,
            late: 0,
            window: 0.0,
            late_windows: 0,
            headroom: 0.0,
            patience: 20.0,
            raised_at: -100.0,
            heat_drop: false,
            cool_since: None,
        }
    }
}

impl AdaptiveQuality {
    pub const MAX_LEVEL: u32 = LADDER.len() as u32 - 1;

    pub fn fog_steps(&self, quality: Quality) -> u32 {
        let base = match quality {
            Quality::High => 16,
            Quality::Medium => 12,
            Quality::Low => 10,
        };
        (base * LADDER[self.level as usize].1 / 4).max(6)
    }

    /// Multiplier on the 3D render resolution.
    pub fn render_scale(&self) -> f32 {
        LADDER[self.level as usize].0
    }

    /// Start-up measurement running: render even if hidden, no frame cap.
    pub fn calibrating(&self) -> bool {
        matches!(self.calibration, Calibration::Measuring { .. })
    }

    pub fn ready(&self) -> bool {
        self.calibration == Calibration::Done
    }

    /// Reference GPU time of a frame at `level` (ms).
    fn reference(&self, level: u32) -> f32 {
        let s = LADDER[level as usize].0;
        FIXED_MS + PER_MPX_MS * self.full_mpx * s * s
    }

    /// Predicted GPU time of a frame at `level` on this machine, now (ms).
    fn cost(&self, level: u32) -> f32 {
        self.heat * self.reference(level)
    }

    /// Sharpest level whose predicted cost fits `budget` ms.
    fn fitting(&self, budget: f32) -> u32 {
        (0..=Self::MAX_LEVEL).find(|&l| self.cost(l) <= budget).unwrap_or(Self::MAX_LEVEL)
    }

    fn reset_window(&mut self) {
        self.frames = 0;
        self.late = 0;
        self.window = 0.0;
    }
}

fn pinned() -> Option<u32> {
    std::env::var("AQ_LEVEL").ok().and_then(|v| v.parse().ok()).map(|l: u32| l.min(AdaptiveQuality::MAX_LEVEL))
}

/// GPU time a frame may take (ms): 80 % of a 60 Hz frame, less when warm.
/// The same at 30 fps (battery, Low Power Mode): the time saved there is
/// energy saved, not spent on pixels. Spent, the GPU stayed almost as busy
/// (8.5 W at 30 fps against 11.6 W at 60, measured).
fn budget(power: &Power) -> f32 {
    let thermal = match power.thermal {
        0 => 1.0,
        1 => 0.9,
        2 => 0.75,
        _ => 0.6,
    };
    0.8 * 1000.0 / 60.0 * thermal
}

/// Start-up: measures this machine behind the curtain.
#[allow(clippy::too_many_arguments)]
fn calibrate(
    time: Res<Time<Real>>,
    config: Res<AppConfig>,
    power: Res<Power>,
    sdf: Res<crate::sdf::DecorSdf>,
    target: Res<SceneTarget>,
    occluded: Res<Occluded>,
    mut aq: ResMut<AdaptiveQuality>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let now = time.elapsed_secs();
    let hidden = occluded.0 || power.screen_off;
    match aq.calibration {
        Calibration::Done => {}
        Calibration::Waiting => {
            // Captures and tests: fixed resolution, level 0 (or AQ_LEVEL).
            if config.screenshot.is_some() || pinned().is_some() {
                aq.level = pinned().unwrap_or(0);
                aq.calibration = Calibration::Done;
                return;
            }
            // Everything drawn at its real cost: shaders, decor, textures.
            if !(crate::intro::pipelines_ready() && sdf.complete && crate::textures::textures_ready()) {
                return;
            }
            // Only a visible window's frames tell the truth (a covered one
            // reads faster than it renders): wait, curtain down, not rendering.
            if hidden {
                return;
            }
            aq.started = now;
            aq.level = 0;
            aq.samples.clear();
            aq.calibration = Calibration::Measuring { skip: 12 };
            if let Ok(mut w) = windows.single_mut() {
                w.present_mode = PresentMode::AutoNoVsync;
            }
        }
        Calibration::Measuring { skip } => {
            if hidden {
                aq.samples.clear();
                aq.calibration = Calibration::Waiting;
                if let Ok(mut w) = windows.single_mut() {
                    w.present_mode = PresentMode::AutoVsync;
                }
                return;
            }
            if now - aq.started > 6.0 {
                finish(&mut aq, &power, &mut windows, "calibration timed out, reference model");
                return;
            }
            if skip > 0 {
                aq.calibration = Calibration::Measuring { skip: skip - 1 };
                return;
            }
            aq.samples.push(time.delta_secs() * 1000.0);
            if aq.samples.len() < 3 * CHUNK {
                return;
            }
            // Median of each third, the fastest kept: something else briefly
            // using the machine only ever slows frames down.
            let mut chunks: Vec<f32> = aq
                .samples
                .chunks_mut(CHUNK)
                .map(|c| {
                    c.sort_by(f32::total_cmp);
                    c[CHUNK / 2]
                })
                .collect();
            aq.samples.clear();
            chunks.sort_by(f32::total_cmp);
            let ms = chunks[0];
            aq.full_mpx = target.full_mpx;
            // A frame faster than the refresh reads too slow: an upper bound,
            // on the safe side.
            aq.heat = (ms / aq.reference(0)).clamp(0.25, 4.0);
            aq.base_heat = aq.heat;
            aq.calibrated_heat = aq.heat;
            let note = format!(
                "{ms:.1} ms at {:.1} Mpx (thirds {:.1}/{:.1}/{:.1}), {:.2}× the reference",
                aq.full_mpx, chunks[0], chunks[1], chunks[2], aq.heat
            );
            finish(&mut aq, &power, &mut windows, &note);
        }
    }
}

fn finish(aq: &mut AdaptiveQuality, power: &Power, windows: &mut Query<&mut Window, With<PrimaryWindow>>, note: &str) {
    if let Ok(mut w) = windows.single_mut() {
        w.present_mode = PresentMode::AutoVsync;
    }
    aq.level = aq.fitting(budget(power));
    aq.calibration = Calibration::Done;
    info!(
        "quality: {note}; level {} ({:.0}% resolution, {:.1} ms predicted, {:.0} fps)",
        aq.level,
        LADDER[aq.level as usize].0 * 100.0,
        aq.cost(aq.level),
        power.awake_fps()
    );
}

/// Follows the cost while awake; moves the resolution only unobserved (or when
/// frames keep missing the refresh).
fn adapt(
    time: Res<Time<Real>>,
    config: Res<AppConfig>,
    power: Res<Power>,
    occluded: Res<Occluded>,
    target: Res<SceneTarget>,
    mut aq: ResMut<AdaptiveQuality>,
    mut fog: Query<&mut VolumetricFog>,
) {
    if let Some(level) = pinned() {
        aq.level = level;
    } else if aq.ready() && config.screenshot.is_none() {
        // Another screen (lid closed, display plugged) or a resized window:
        // the picture changes anyway, the level is chosen again at once.
        if target.full_mpx > 0.0 && (target.full_mpx / aq.full_mpx - 1.0).abs() > 0.05 {
            aq.full_mpx = target.full_mpx;
            let level = aq.fitting(budget(&power));
            info!("quality: {:.1} Mpx at full resolution now -> level {level}", aq.full_mpx);
            aq.level = level;
            aq.pending = None;
        }
        step(&time, &power, occluded.0, &mut aq);
    }
    let steps = aq.fog_steps(config.quality);
    for mut f in &mut fog {
        if f.step_count != steps {
            f.step_count = steps;
        }
    }
}

fn step(time: &Time<Real>, power: &Power, occluded: bool, aq: &mut AdaptiveQuality) {
    let now = time.elapsed_secs();
    // A drop forced by heat holds until macOS has reported a normal temperature
    // for 5 min; then the start-up measurement counts again (and a better
    // level is applied, as always, when nobody is looking).
    if aq.heat_drop {
        if power.thermal == 0 {
            if now - *aq.cool_since.get_or_insert(now) > 300.0 {
                info!("quality: cooled down, back to the start-up measurement");
                aq.base_heat = aq.calibrated_heat;
                aq.heat_drop = false;
                aq.cool_since = None;
            }
        } else {
            aq.cool_since = None;
        }
    }
    // Nobody looking (wallpaper asleep, window hidden): the moment to switch.
    if power.mode != Mode::Awake || occluded {
        if let Some(level) = aq.pending.take() {
            if level < aq.level {
                aq.raised_at = now;
            }
            info!("quality: level {} -> {level} while nobody is looking", aq.level);
            aq.level = level;
        }
        aq.reset_window();
        aq.late_windows = 0;
        return;
    }
    // Measure at full speed only (not during the wake-up ramp).
    if power.awake < 0.99 {
        aq.reset_window();
        return;
    }
    let dt = time.delta_secs();
    let fps = power.target_fps.min(60.0);
    aq.frames += 1;
    aq.window += dt;
    if dt > 1.4 / fps {
        aq.late += 1;
    }
    if aq.window < 2.0 {
        return;
    }
    let mean_ms = aq.window * 1000.0 / aq.frames as f32;
    let late = aq.late as f32 / aq.frames as f32;
    let window = aq.window;
    aq.reset_window();

    #[cfg(target_os = "macos")]
    let busy = crate::macos::gpu_busy();
    #[cfg(not(target_os = "macos"))]
    let busy: Option<f32> = None;
    // Frames late with the GPU saturated for 4 s: the resolution is the right
    // knob, and the frame time is what a frame costs now. A shorter stretch
    // (a window dragged, another app's animation) changes nothing, and a busy
    // GPU alone says little: 93–95 % in the wallpaper at a level that holds
    // 60 fps.
    let gpu_bound = busy.is_none_or(|b| b > 0.9);
    aq.late_windows = if late > 0.1 && gpu_bound { aq.late_windows + 1 } else { 0 };
    if aq.late_windows >= 2 {
        let ms = busy.unwrap_or(1.0) * mean_ms;
        aq.heat = aq.heat.max((ms / aq.reference(aq.level)).clamp(aq.base_heat, 4.0));
    } else if aq.late_windows == 0 {
        // The frame fits: back towards the start-up measurement (~20 s).
        aq.heat += (aq.base_heat - aq.heat) * 0.1;
    }
    if aq.late_windows >= 2 && aq.level < AdaptiveQuality::MAX_LEVEL {
        // Visible, but stuttering is worse. The smallest step that ends it:
        // the frame time already includes any throttling, so no thermal margin
        // on top (that one made it jump from 67 % straight to 50 %).
        let level = aq.fitting(0.95 * 1000.0 / fps).max(aq.level + 1);
        if now - aq.raised_at < 60.0 {
            aq.patience = (aq.patience * 2.0).min(600.0);
        }
        info!(
            "quality: frames late for 4 s ({:.0}% late, GPU {:.0}%, thermal {}) -> level {level}",
            late * 100.0,
            busy.unwrap_or(1.0) * 100.0,
            power.thermal
        );
        aq.level = level;
        aq.pending = None;
        aq.late_windows = 0;
        aq.headroom = 0.0;
        // Not retried: until the next launch, or until the Mac has cooled
        // down if heat was the cause.
        aq.base_heat = aq.base_heat.max(aq.heat);
        aq.heat_drop = power.thermal > 0;
        aq.cool_since = None;
        return;
    }
    let want = aq.fitting(budget(power));

    if want > aq.level {
        aq.headroom = 0.0;
        aq.pending = Some(want);
    } else if want < aq.level {
        aq.headroom += window;
        if aq.headroom >= aq.patience {
            aq.pending = Some(want);
        }
    } else {
        aq.headroom = 0.0;
        aq.pending = None;
    }
}

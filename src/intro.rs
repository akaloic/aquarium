//! Start-up: the screen stays black while the shaders compile (pipelines are
//! specialised on first use: every material, including the pooled food flakes,
//! bubbles and sand grains, is on screen during these frames), then the
//! aquarium fades in — no hitch, no pop-in.

use std::sync::atomic::{AtomicUsize, Ordering};

use bevy::{
    prelude::*,
    render::{Render, RenderApp, RenderSystems, render_resource::PipelineCache},
};

use crate::{config::AppConfig, sdf::DecorSdf};

static PENDING: AtomicUsize = AtomicUsize::new(usize::MAX);

/// No render pipeline waiting to be compiled (as of the last render update).
pub fn pipelines_ready() -> bool {
    PENDING.load(Ordering::Relaxed) == 0
}

#[derive(Component)]
struct Curtain {
    ready_frames: u32,
    fade: f32,
}

pub struct IntroPlugin;

impl Plugin for IntroPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_curtain).add_systems(Update, lift_curtain);
        app.sub_app_mut(RenderApp).add_systems(
            Render,
            count_pending.in_set(RenderSystems::Cleanup),
        );
    }
}

fn count_pending(cache: Res<PipelineCache>, mut logged: Local<usize>) {
    PENDING.store(cache.waiting_pipelines().count(), Ordering::Relaxed);
    // `AQ_MEM=1`: how many pipelines were specialised (each is GPU code).
    let n = cache.pipelines().count();
    if std::env::var("AQ_MEM").is_ok() && n != *logged && PENDING.load(Ordering::Relaxed) == 0 {
        *logged = n;
        info!("render pipelines: {n}");
    }
}

fn spawn_curtain(mut commands: Commands, config: Res<AppConfig>) {
    if config.screenshot.is_some() {
        return;
    }
    commands.spawn((
        Name::new("curtain"),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::BLACK),
        GlobalZIndex(100),
        Curtain { ready_frames: 0, fade: 0.0 },
    ));
}

fn lift_curtain(
    mut commands: Commands,
    time: Res<Time<Real>>,
    sdf: Res<DecorSdf>,
    quality: Res<crate::quality::AdaptiveQuality>,
    occluded: Res<crate::wallpaper::Occluded>,
    power: Res<crate::power::Power>,
    mut shown: Local<f32>,
    mut curtain: Query<(Entity, &mut Curtain, &mut BackgroundColor)>,
) {
    let Ok((e, mut c, mut bg)) = curtain.single_mut() else {
        return;
    };
    // Shaders compiled, decor known, and the resolution chosen (the quality
    // calibration runs behind the curtain).
    let ready = pipelines_ready() && sdf.complete && crate::textures::textures_ready() && quality.ready();
    c.ready_frames = if ready { c.ready_frames + 1 } else { 0 };
    // Stable for a few frames (new pipelines can be queued as things appear),
    // or give up waiting after 15 s on screen (a covered window waits to be
    // seen to render and measure anything).
    if !occluded.0 && !power.screen_off {
        *shown += time.delta_secs();
    }
    if c.ready_frames > 20 || *shown > 15.0 || c.fade > 0.0 {
        c.fade += time.delta_secs() / 1.2;
        let a = 1.0 - c.fade.min(1.0);
        bg.0 = Color::BLACK.with_alpha(a * a * (3.0 - 2.0 * a));
        if c.fade >= 1.0 {
            info!("ready in {:.1} s", time.elapsed_secs());
            commands.entity(e).despawn();
        }
    }
}

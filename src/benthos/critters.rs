//! Flatfish and starfish. Their fins and arms are animated on the GPU
//! (critter_anim.wgsl) from a phase that advances with the speed.

use std::f32::consts::TAU;

use bevy::{mesh::MeshTag, prelude::*};
use rand::{RngExt, rng};

use super::{
    Crawler, CritterMaterial, CritterParams, SandPuffs, Threats, critter_bundle, critter_material, disc_is_free, lin,
    mesh, pack_tag, wander_target_with_room,
};
use crate::{
    interaction::Stimuli,
    scape::sand_height,
    sdf::DecorSdf,
    tank::{HALF_D, HALF_W, WATER_Y},
};

pub fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CritterMaterial>>,
) {
    let mut r = rng();
    let on_sand = |x: f32, z: f32| Vec3::new(x, sand_height(x, z), z);

    // --- Flatfish ---
    for (x, z, l) in [(0.52f32, 0.16f32, 0.085f32), (-0.52, -0.02, 0.07)] {
        let mat = critter_material(
            &mut materials,
            CritterParams {
                kind: Vec4::new(3.0, l, 14.0, x),
                color_a: lin(0.62, 0.52, 0.38),
                color_b: lin(0.9, 0.45, 0.15),
                look: Vec4::new(0.0, 40.0, 0.55, 0.0),
                ..default()
            },
            StandardMaterial::default(),
        );
        let p = on_sand(x, z);
        commands.spawn((
            critter_bundle("flatfish", meshes.add(mesh::flatfish(l)), mat, Transform::from_translation(p)),
            Glider {
                // Sand and gentle slopes only (it glides round the rocks, never
                // onto them: stuck up there it would never find sand again).
                body: Crawler::new(p, Vec3::new(-x.signum(), 0.0, 0.3), 0.004, 0.8, false).swimmer(0.8),
                mode: GlideMode::Rest,
                timer: r.random_range(4.0..20.0),
                think: 0.0,
                target: p,
                phase: 0.0,
                size: l,
                buried: 0.5,
                alert: 0.0,
                logged: GlideMode::Rest,
                stuck: false,
            },
        ));
    }

    // --- Starfish: one on the front glass (we see its tube feet), one on a rock ---
    let star = |materials: &mut Assets<CritterMaterial>, radius: f32, a: Vec4, b: Vec4, seed: f32| {
        critter_material(
            materials,
            CritterParams {
                kind: Vec4::new(4.0, radius * 2.0, 4.0, seed),
                color_a: a,
                color_b: b,
                color_c: lin(0.95, 0.72, 0.5),
                look: Vec4::new(0.0, 40.0, 0.6, 0.0),
            },
            StandardMaterial::default(),
        )
    };
    let glass_star = star(&mut materials, 0.045, lin(0.95, 0.35, 0.08), lin(0.5, 0.05, 0.03), 1.0);
    let p = Vec3::new(-0.3, 0.22, HALF_D);
    commands.spawn((
        critter_bundle("starfish", meshes.add(mesh::starfish(0.045)), glass_star, Transform::from_translation(p)),
        Star {
            body: Crawler {
                up: Vec3::NEG_Z,
                only_glass: true,
                ..Crawler::new(p, Vec3::X, 0.004, -1.0, true)
            },
            target: p,
            think: 0.0,
            alert: 0.0,
        },
    ));
    let rock_star = star(&mut materials, 0.035, lin(0.85, 0.12, 0.3), lin(0.98, 0.85, 0.3), 2.0);
    let p = Vec3::new(-0.05, 0.3, 0.02);
    commands.spawn((
        critter_bundle("starfish", meshes.add(mesh::starfish(0.035)), rock_star, Transform::from_translation(p)),
        Star {
            body: Crawler::new(p, Vec3::X, 0.004, 0.2, false),
            target: p,
            think: 1.0,
            alert: 0.0,
        },
    ));
}

// ---------------------------------------------------------------------------
// Flatfish: glide over the sand, settle and bury themselves
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum GlideMode {
    Rest,
    Glide,
    Startle,
}

#[derive(Component)]
pub struct Glider {
    body: Crawler,
    mode: GlideMode,
    timer: f32,
    think: f32,
    target: Vec3,
    phase: f32,
    size: f32,
    /// 0 = on the sand, 1 = fully buried.
    buried: f32,
    alert: f32,
    logged: GlideMode,
    /// Settled where there was no room (don't keep trying to move).
    stuck: bool,
}

fn glide(
    time: &Time,
    sdf: &DecorSdf,
    stimuli: &Stimuli,
    threats: &Threats,
    puffs: &mut SandPuffs,
    g: &mut Glider,
    tr: &mut Transform,
    tag: &mut MeshTag,
) {
    let dt = time.delta_secs().min(0.05);
    let mut r = rng();
    if !sdf.complete {
        return;
    }
    g.timer -= dt;
    g.think -= dt;
    let pos = g.body.pos;
    if g.think <= 0.0 {
        g.think = 0.25;
        if super::log_critters() && g.logged != g.mode {
            info!(
                "flatfish: -> {:?} at ({:.2}, {:.2}) target ({:.2}, {:.2}), buried {:.2}, free {}",
                g.mode,
                pos.x,
                pos.z,
                g.target.x,
                g.target.z,
                g.buried,
                disc_is_free(sdf, pos, g.size * 0.4)
            );
            g.logged = g.mode;
        }
        let reach = 0.1;
        if let Some((_, away)) = threats.near(stimuli, pos, reach).or(threats.startle.then_some((0.0, g.body.facing))) {
            if g.mode != GlideMode::Startle {
                g.mode = GlideMode::Startle;
                g.target = wander_target_with_room(sdf, pos + away * 0.25, 0.12, true, g.size * 0.4);
                g.timer = 4.0;
                g.alert = 1.0;
                puffs.requests.push((pos, if g.buried > 0.3 { 1.0 } else { 0.4 }));
            }
        } else {
            g.alert = (g.alert - 0.05).max(0.0);
            // Lying against a rock (or under a pebble), or on top of one: move to
            // open sand. A flatfish lies on sand only.
            let on_rock = pos.y > sand_height(pos.x, pos.z) + 0.008;
            let cramped = g.mode == GlideMode::Rest && (on_rock || !g.stuck && !disc_is_free(sdf, pos, g.size * 0.4));
            match g.mode {
                GlideMode::Rest if g.timer <= 0.0 || cramped => {
                    g.stuck = false;
                    g.mode = GlideMode::Glide;
                    g.target = wander_target_with_room(sdf, pos, 0.22, true, g.size * 0.4);
                    g.timer = 20.0;
                    if g.buried > 0.3 {
                        puffs.requests.push((pos, 0.7));
                    }
                }
                // Arrived over a rock (a target it couldn't reach): not a place to rest.
                GlideMode::Glide | GlideMode::Startle
                    if (pos.distance(g.target) < 0.02 || g.timer <= 0.0) && on_rock =>
                {
                    g.target = wander_target_with_room(sdf, pos, 0.3, true, g.size * 0.4);
                    g.timer = 20.0;
                }
                GlideMode::Glide | GlideMode::Startle if pos.distance(g.target) < 0.02 || g.timer <= 0.0 => {
                    g.mode = GlideMode::Rest;
                    g.timer = r.random_range(10.0..40.0);
                    // Nowhere better found: settle here anyway until the next outing.
                    g.stuck = !disc_is_free(sdf, pos, g.size * 0.4);
                    // Settling kicks up sand, then it wriggles in.
                    puffs.requests.push((pos, 0.8));
                }
                _ => {}
            }
        }
    }

    let speed = match g.mode {
        GlideMode::Rest => 0.0,
        GlideMode::Glide => 0.05,
        GlideMode::Startle => 0.16,
    };
    let to = g.target - pos;
    let dir = to.normalize_or_zero() * (to.length() / 0.04).min(1.0);
    let face = if dir.length() > 0.2 { dir } else { g.body.facing };
    // At rest it stops within a fraction of a second (no sliding along).
    let accel = if g.mode == GlideMode::Rest { 0.4 } else { speed * 1.5 };
    if !g.body.step(sdf, dir * speed, face, accel, 2.5, dt) && g.mode != GlideMode::Rest {
        // Blocked (glass, a cliff of rock): go somewhere else.
        g.target = wander_target_with_room(sdf, pos, 0.3, true, g.size * 0.4);
    }
    // Lift off while swimming, sink into the sand at rest.
    let moving = g.mode != GlideMode::Rest;
    let hover_goal = if moving { g.size * 0.12 } else { 0.0 };
    g.body.hover += (hover_goal - g.body.hover) * (1.0 - (-dt * 2.0).exp());
    let bury_goal = if moving { 0.0 } else { 0.85 };
    let rate = if moving { 3.0 } else { 0.5 };
    g.buried += (bury_goal - g.buried) * (1.0 - (-dt * rate).exp());
    g.body.sink = g.buried * g.size * 0.035;
    let v = g.body.vel.length();
    // The wave travels faster when swimming fast; a slow ripple at rest (breathing).
    g.phase -= dt * (1.2 + v / g.size * 9.0);
    let amplitude = if moving { 0.4 + 0.6 * (v / speed.max(1e-3)).min(1.0) } else { 0.12 };
    *tr = g.body.transform(1.0);
    let packed = pack_tag(g.phase, amplitude, g.buried, g.alert);
    if tag.0 != packed {
        tag.0 = packed;
    }
}

impl Glider {
    /// (mode, contact point, up, speed, buried) for the physics test.
    pub fn probe(&self) -> (&'static str, Vec3, Vec3, f32, f32) {
        let mode = match self.mode {
            GlideMode::Rest => "rest",
            GlideMode::Glide => "glide",
            GlideMode::Startle => "startle",
        };
        (mode, self.body.pos, self.body.up, self.body.vel.length(), self.buried)
    }
}

impl Star {
    /// (contact point, up, speed) for the physics test.
    pub fn probe(&self) -> (Vec3, Vec3, f32) {
        (self.body.pos, self.body.up, self.body.vel.length())
    }
}

pub fn flatfish(
    time: Res<Time>,
    sdf: Res<DecorSdf>,
    stimuli: Res<Stimuli>,
    threats: Res<Threats>,
    mut puffs: ResMut<SandPuffs>,
    mut q: Query<(&mut Glider, &mut Transform, &mut MeshTag)>,
) {
    for (mut g, mut tr, mut tag) in &mut q {
        glide(&time, &sdf, &stimuli, &threats, &mut puffs, &mut g, &mut tr, &mut tag);
    }
}

// ---------------------------------------------------------------------------
// Starfish: a few millimetres per second, anywhere (glass included)
// ---------------------------------------------------------------------------

#[derive(Component)]
pub struct Star {
    body: Crawler,
    target: Vec3,
    think: f32,
    alert: f32,
}

/// The starfish on the front pane: moves in the plane of the glass, between
/// the sand and the water line, arms towards the tank.
fn on_glass(
    time: &Time,
    sdf: &DecorSdf,
    stimuli: &Stimuli,
    threats: &Threats,
    s: &mut Star,
    tr: &mut Transform,
    tag: &mut MeshTag,
) {
    let dt = time.delta_secs().min(0.05);
    let mut r = rng();
    let radius = 0.045;
    let x_max = HALF_W - crate::tank::CORNER_R - radius;
    let bounds = |x: f32| (sand_height(x, HALF_D) + radius + 0.02, WATER_Y - radius - 0.04);
    s.think -= dt;
    let pos = s.body.pos;
    if s.think <= 0.0 {
        s.think = 0.5;
        let near = threats.near(stimuli, pos, 0.05).is_some();
        s.alert = if near { 1.0 } else { (s.alert - 0.05).max(0.0) };
        if Vec2::new(pos.x, pos.y).distance(Vec2::new(s.target.x, s.target.y)) < 0.01 {
            let a = r.random_range(0.0..TAU);
            let d = r.random_range(0.04..0.12);
            let x = (pos.x + a.cos() * d).clamp(-x_max, x_max);
            let (lo, hi) = bounds(x);
            s.target = Vec3::new(x, (pos.y + a.sin() * d).clamp(lo, hi), HALF_D);
        }
    }
    let to = Vec3::new(s.target.x - pos.x, s.target.y - pos.y, 0.0);
    let dir = to.normalize_or_zero() * (to.length() / 0.01).min(1.0);
    let speed = if s.alert > 0.5 { 0.0 } else { 0.004 };
    let dv = (dir * speed - s.body.vel).clamp_length_max(0.01 * dt);
    s.body.vel += dv;
    let mut p = pos + s.body.vel * dt;
    p.x = p.x.clamp(-x_max, x_max);
    let (lo, hi) = bounds(p.x);
    p.y = p.y.clamp(lo, hi);
    p.z = HALF_D;
    s.body.pos = p;
    s.body.up = Vec3::NEG_Z;
    if dir != Vec3::ZERO {
        let angle = s.body.facing.angle_between(dir);
        let k = if angle > 1e-4 { (0.3 * dt / angle).min(1.0) } else { 1.0 };
        s.body.facing = s.body.facing.slerp(dir, k).normalize_or(Vec3::X);
    }
    let _ = sdf;
    *tr = s.body.transform(1.0);
    let packed = pack_tag(0.0, 0.0, 0.0, s.alert);
    if tag.0 != packed {
        tag.0 = packed;
    }
}

pub fn starfish(
    time: Res<Time>,
    sdf: Res<DecorSdf>,
    stimuli: Res<Stimuli>,
    threats: Res<Threats>,
    mut q: Query<(&mut Star, &mut Transform, &mut MeshTag)>,
) {
    let dt = time.delta_secs().min(0.05);
    let mut r = rng();
    if !sdf.complete {
        return;
    }
    for (mut s, mut tr, mut tag) in &mut q {
        if s.body.only_glass {
            on_glass(&time, &sdf, &stimuli, &threats, &mut s, &mut tr, &mut tag);
            continue;
        }
        s.think -= dt;
        let pos = s.body.pos;
        if s.think <= 0.0 {
            s.think = 0.5;
            // Touched: curls its arms up.
            let near = threats.near(&stimuli, pos, 0.05).is_some();
            s.alert = if near { 1.0 } else { (s.alert - 0.05).max(0.0) };
            if pos.distance(s.target) < 0.01 {
                // Next goal: along its current surface.
                let tangent = {
                    let a = r.random_range(0.0..TAU);
                    let t = s.body.up.any_orthonormal_vector();
                    let b = s.body.up.cross(t);
                    t * a.cos() + b * a.sin()
                };
                s.target = pos + tangent * r.random_range(0.04..0.12);
            }
        }
        let to = s.target - pos;
        let dir = to.normalize_or_zero();
        let speed = if s.alert > 0.5 { 0.0 } else { 0.004 };
        let face = if dir != Vec3::ZERO { dir } else { s.body.facing };
        if !s.body.step(&sdf, dir * speed, face, 0.01, 0.3, dt) {
            s.target = pos;
        }
        *tr = s.body.transform(1.0);
        let packed = pack_tag(0.0, 0.0, 0.0, s.alert);
        if tag.0 != packed {
            tag.0 = packed;
        }
    }
}

//! Boids steering with Perlin wander and ray-cast obstacle avoidance.

use std::sync::OnceLock;

use bevy::{mesh::MeshTag, prelude::*};
use noise::{NoiseFn, Perlin};

use super::{Fish, pack_swim_tag};
use crate::interaction::Stimuli;
use crate::{
    scape::sand_height,
    sdf::DecorSdf,
    tank::{HALF_D, HALF_W, WATER_Y},
};

fn perlin() -> &'static Perlin {
    static P: OnceLock<Perlin> = OnceLock::new();
    P.get_or_init(|| Perlin::new(0xB01D))
}

/// Closest point of segment [a, b] to p, and the distance to it.
fn closest_on_segment(a: Vec3, b: Vec3, p: Vec3) -> (Vec3, f32) {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-8)).clamp(0.0, 1.0);
    let c = a + ab * t;
    (c, c.distance(p))
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub struct Other {
    entity: Entity,
    species: usize,
    pos: Vec3,
    vel: Vec3,
    length: f32,
    panic: f32,
}

pub fn steer(
    time: Res<Time>,
    stimuli: Res<Stimuli>,
    sdf: Res<DecorSdf>,
    mut snapshot: Local<Vec<Other>>,
    mut fish: Query<(Entity, &mut Fish, &mut Transform)>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.05);
    if dt <= 0.0 {
        return;
    }
    let started = std::time::Instant::now();
    let t = time.elapsed_secs_f64();
    // Reused every frame: no allocation in the simulation loop.
    snapshot.clear();
    snapshot.extend(fish.iter().map(|(entity, f, tr)| Other {
        entity,
        species: f.species,
        pos: tr.translation,
        vel: f.velocity,
        length: f.length,
        panic: f.panic,
    }));
    let noise = perlin();
    let smooth = |rate: f32| 1.0 - (-dt * rate).exp();

    for (entity, mut f, mut tr) in &mut fish {
        let b = f.behaviour;
        let mut events = 0u8;
        let pos = tr.translation;
        let vel = f.velocity;
        let speed = vel.length().max(1e-4);
        let fwd = vel / speed;

        // --- Threat: the cursor "finger" poking into the water ---
        if let Some(c) = &stimuli.cursor {
            let (closest, d) = closest_on_segment(c.from, c.to, pos);
            // A moving hand frightens from further away than a still one.
            let reach = 0.07 + 0.08 * (c.speed / 0.2).min(1.0);
            if d < reach {
                let threat = smoothstep(reach, reach * 0.35, d);
                if threat > f.panic {
                    f.panic = threat;
                }
                let away = (pos - closest).normalize_or(-fwd);
                // Dart away from the finger and ahead of its motion.
                let flee = (away + c.velocity.normalize_or_zero() * 0.5).normalize_or(away);
                f.flee = f.flee.lerp(flee, smooth(20.0)).normalize_or(flee);
            }
        }

        // --- Boids (neighbours fade in and out of view smoothly) ---
        let mut separation = Vec3::ZERO;
        let (mut align, mut center, mut n) = (Vec3::ZERO, Vec3::ZERO, 0.0);
        let mut alarm: f32 = 0.0;
        let mut alarm_dir = Vec3::ZERO;
        for o in snapshot.iter() {
            if o.entity == entity {
                continue;
            }
            let d = pos - o.pos;
            let dist = d.length();
            let r_sep = 0.7 * (f.length + o.length) + 0.012;
            if dist < r_sep && dist > 1e-5 {
                let x = 1.0 - dist / r_sep;
                separation += d / dist * x * x;
            }
            if o.species == f.species && dist < b.view_radius {
                let w = 1.0 - smoothstep(0.6 * b.view_radius, b.view_radius, dist);
                align += o.vel * w;
                center += o.pos * w;
                n += w;
                // Panic spreads through the school like a wave.
                if o.panic > 0.3 {
                    let a = o.panic * w * 0.85;
                    if a > alarm {
                        alarm = a;
                    }
                    alarm_dir += o.vel.normalize_or_zero() * o.panic * w;
                }
            }
        }
        if alarm > f.panic {
            f.panic += (alarm - f.panic) * smooth(6.0);
            f.flee = f.flee.lerp(alarm_dir.normalize_or(f.flee), smooth(6.0)).normalize_or(fwd);
        }
        let panic = f.panic;
        let feeding_target = if f.satiety < 1.0 && panic < 0.4 {
            // Drifting flakes only: what lies on the ground is for the bottom dwellers.
            stimuli
                .food
                .iter()
                .filter(|f| !f.resting)
                .map(|f| (f.pos, f.pos.distance(pos)))
                .filter(|&(_, d)| d < 0.5)
                .min_by(|a, b| a.1.total_cmp(&b.1))
        } else {
            None
        };

        let max_acc = b.accel * f.urge.max(1.0) * (1.0 + 2.5 * panic) * if feeding_target.is_some() { 1.6 } else { 1.0 };
        let mut force = separation * b.separation * 0.8;
        if n > 1e-3 {
            let weight = n.min(1.0);
            // After a scare, schools pull tighter together.
            let cohesion = b.cohesion * (1.0 + 1.5 * panic);
            force += (align / n - vel) * b.alignment * 1.5 * weight;
            force += (center / n - pos) * cohesion * 0.6 * weight;
        }

        // --- Wander (Perlin), slightly flattened: fish mostly swim level ---
        let s = f.seed;
        let tw = t * 0.18;
        let wander = Vec3::new(
            noise.get([s, tw, 0.5]) as f32,
            noise.get([s + 17.3, tw * 0.8, 1.5]) as f32 * 0.35,
            noise.get([s + 41.1, tw, 2.5]) as f32,
        );
        force += wander * b.wander * b.accel;
        // Speed urge: slow drifts, sometimes a burst.
        let urge_n = noise.get([s + 7.7, t * 0.12, 3.5]) as f32;
        f.urge = (1.0 + urge_n * 1.3).clamp(0.35, 2.4);

        let mut desired_speed = b.cruise * f.urge;
        if let Some((food, d)) = feeding_target {
            // Go for the flake, slowing down to snap it.
            let mouth = pos + fwd * f.length * 0.45;
            let to = food - mouth;
            force += to.normalize_or_zero() * max_acc * 1.5;
            desired_speed = (b.cruise * 2.2).min(b.cruise * 0.6 + d * 2.5);
        } else {
            // --- Preferred depth band and home ---
            let floor = sand_height(pos.x, pos.z);
            let (lo, hi) = ((floor + 0.03).max(b.depth.0), b.depth.1);
            if pos.y < lo {
                force.y += (lo - pos.y) * 4.0;
            } else if pos.y > hi {
                force.y -= (pos.y - hi) * 4.0;
            }
            if let Some(home) = f.home {
                let to = home + Vec3::Y * 0.03 - pos;
                let d = to.length();
                if d > 0.06 {
                    force += to / d * (d - 0.06) * 6.0 * (1.0 - panic);
                }
            }
        }
        if panic > 0.0 {
            force += f.flee * max_acc * 2.0 * panic;
            desired_speed = desired_speed + (b.max_speed * 1.5 - desired_speed) * panic;
        }

        // --- Decor avoidance through the distance field ---
        // Clearance the body needs, and how far ahead the fish pays attention.
        let clearance = f.length * 0.55 + 0.01;
        let range = clearance + 0.035 + speed * 0.45;
        let (d_here, n_here) = sdf.sample(pos);
        if d_here < range {
            // Gentle push away from whatever is close to the body.
            let x = 1.0 - ((d_here - clearance) / (range - clearance)).clamp(0.0, 1.0);
            force += n_here * x * x * b.accel * 3.0;
        }
        let look = f.length * 1.5 + speed * 1.2 + 0.03;
        let (d_ahead, n_ahead) = sdf.sample(pos + fwd * look);
        if d_ahead < range {
            let x = 1.0 - ((d_ahead - clearance) / (range - clearance)).clamp(0.0, 1.0);
            let into = fwd.dot(n_ahead).min(0.0);
            // Head-on: pick a side once (the more open one) and keep it.
            let side = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
            if f.avoid_side == 0.0 {
                let open = |s: f32| sdf.distance(pos + (fwd * 0.5 + side * s).normalize() * look);
                f.avoid_side = if open(-1.0) > open(1.0) { -1.0 } else { 1.0 };
            }
            f.avoid_timer = 0.6;
            // Slide along the surface in a smooth curve, slowing down progressively.
            let tangent = (fwd - n_ahead * into + side * f.avoid_side * (-into) * 0.6).normalize_or(side * f.avoid_side);
            force += (tangent - fwd) * x * b.accel * 5.0 + n_ahead * x * x * b.accel * 2.0;
            desired_speed *= 1.0 - 0.55 * x * (-into);
            events |= 1 | if f.avoid_side > 0.0 { 2 } else { 0 };
        } else if f.avoid_timer > 0.0 {
            f.avoid_timer -= dt;
            if f.avoid_timer <= 0.0 {
                f.avoid_side = 0.0;
            }
        }

        // --- Integrate: smoothed steering, rate-limited turns ---
        // Neighbour and whisker forces flicker from frame to frame; a fish's body
        // integrates them over a fraction of a second. A panicked fish reacts fast.
        let force = force.clamp_length_max(max_acc);
        f.steer = f.steer.lerp(force, smooth(5.0 + 20.0 * panic));
        let steer = f.steer;

        // Heading: aim where the steering points half a second ahead.
        let mut want = (vel + steer * 0.6).normalize_or(fwd);
        want.y = want.y.clamp(-0.4, 0.4);
        let want = want.normalize();
        let axis = fwd.cross(want);
        let sin = axis.length();
        let angle = sin.atan2(fwd.dot(want));
        let turning = if f.ang_vel.y.abs() > 0.05 {
            f.ang_vel.y.signum()
        } else if f.avoid_side < 0.0 {
            1.0
        } else {
            -1.0
        };
        let axis = if sin > 1e-5 && (angle < 2.4 || axis.y * turning > 0.0) {
            axis / sin
        } else {
            // (Almost) straight behind: keep turning the way we already are,
            // instead of flipping sides on tiny changes.
            Vec3::Y * turning
        };
        let max_turn = b.turn_rate * (0.6 + 0.4 * f.urge.min(2.0)) * (1.0 + 2.0 * panic);
        if angle * 3.0 > max_turn {
            events |= 32;
        }
        let mut target_w = axis * (angle * 3.0).min(max_turn);
        // Fish pitch much more lazily than they yaw.
        target_w = Vec3::new(target_w.x * 0.5, target_w.y, target_w.z * 0.5);
        // Angular velocity changes smoothly (no bang-bang turning).
        f.ang_vel = f.ang_vel.lerp(target_w, smooth(8.0 + 20.0 * panic));
        let dir = (Quat::from_scaled_axis(f.ang_vel * dt) * fwd).normalize();
        // Fish rarely swim steeply up or down: soft limit on the pitch (on the
        // sine of the angle, whatever the frame rate: at most 27°).
        let knee = 0.32;
        let dir = if dir.y.abs() > knee {
            let sin = dir.y.signum() * (knee + 0.13 * ((dir.y.abs() - knee) / 0.13).tanh());
            let flat = Vec3::new(dir.x, 0.0, dir.z).normalize_or(fwd.with_y(0.0).normalize_or(Vec3::Z));
            flat * (1.0 - sin * sin).sqrt() + Vec3::Y * sin
        } else {
            dir
        };

        let top_speed = b.max_speed * (1.0 + 0.6 * panic);
        let along = steer.dot(fwd);
        let acc = ((desired_speed - speed) * (2.0 + 6.0 * panic) + along * 0.5).clamp(-max_acc, max_acc);
        let new_speed = (speed + acc * dt).clamp(b.cruise * 0.25, top_speed);
        let new_vel = dir * new_speed;

        let mut p = pos + new_vel * dt;
        // Containment: never into the decor (glass, sand, rocks, surface).
        // Close to it, this frame's motion loses its inward part — the fish
        // slides along instead of entering — and the steering takes the hit so
        // the heading turns away smoothly. Already too close: eased out.
        let min_clear = f.length * 0.3 + 0.004;
        let (d_new, n_new) = sdf.sample(p);
        if d_new < min_clear {
            events |= 16;
            let into = (p - pos).dot(n_new).min(0.0);
            p -= n_new * into;
            let (d2, n2) = sdf.sample(p);
            if d2 < min_clear {
                p += n2 * (min_clear - d2).min(0.003);
            }
            f.steer += n_new * max_acc * 2.0;
        }
        let half_len = f.length * 0.5;
        p.x = p.x.clamp(-HALF_W + half_len, HALF_W - half_len);
        p.z = p.z.clamp(-HALF_D + half_len, HALF_D - half_len);
        p.y = p.y.min(WATER_Y - 0.01);

        // Yaw rate (positive = turning left) for body curl and banking.
        f.yaw_rate += (f.ang_vel.y - f.yaw_rate) * smooth(6.0);
        f.panic = (f.panic - dt / 2.2).max(0.0);
        f.satiety = (f.satiety - dt / 45.0).max(0.0);
        f.gulp = (f.gulp - dt * 4.0).max(0.0);

        f.velocity = new_vel;
        f.events = events;
        let bank = (-f.yaw_rate * 0.12).clamp(-0.35, 0.35);
        let up = Quat::from_axis_angle(dir, bank) * Vec3::Y;
        let scale = tr.scale;
        *tr = Transform::from_translation(p).looking_to(dir, up).with_scale(scale);
    }
    // `AQ_CPU=1`: cost of the whole fish simulation step.
    if crate::cputime::enabled() {
        crate::cputime::add_steer(started.elapsed());
    }
}

/// Tail beat: frequency and amplitude follow the speed, the body curls into turns.
pub fn animate(time: Res<Time>, mut fish: Query<(&mut Fish, &mut MeshTag)>) {
    let dt = time.delta_secs().clamp(0.0, 0.05);
    for (mut f, mut tag) in &mut fish {
        let speed = f.velocity.length();
        let rel = speed / f.length;
        let hz = (0.9 + 1.5 * rel).min(9.0);
        let target_omega = std::f32::consts::TAU * hz;
        f.omega += (target_omega - f.omega) * (1.0 - (-dt * (3.0 + 12.0 * f.panic)).exp());
        let target_amp = (0.3 + 0.7 * (speed / f.behaviour.max_speed).clamp(0.0, 1.0)
            + 0.25 * (f.yaw_rate.abs() / f.behaviour.turn_rate).min(1.0)
            + 0.5 * f.panic)
            .min(1.0);
        f.amplitude += (target_amp - f.amplitude) * (1.0 - (-dt * (4.0 + 12.0 * f.panic)).exp());
        f.phase = (f.phase + f.omega * dt).rem_euclid(std::f32::consts::TAU);
        let turn = (-f.yaw_rate / f.behaviour.turn_rate).clamp(-1.0, 1.0) * 0.8;
        let packed = pack_swim_tag(f.phase, f.amplitude, f.omega, turn);
        if tag.0 != packed {
            tag.0 = packed;
        }
    }
}

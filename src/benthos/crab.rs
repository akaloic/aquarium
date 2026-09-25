//! The crab: walks sideways over sand and rocks with procedural inverse-
//! kinematics legs (feet stay planted, alternating tetrapod gait whose step
//! rate follows the speed), picks at the ground with its claws, eats the food
//! that reaches the bottom, and dashes to the nearest hiding place under a rock
//! when the cursor or a big fish comes close — or when the wallpaper wakes up.

use bevy::{mesh::MeshTag, prelude::*};
use rand::{RngExt, rng};

use super::{
    Crawler, CritterMaterial, CritterParams, HideSpots, SandPuffs, Threats, away_from_rocks, critter_bundle,
    critter_material, lin, mesh, pack_tag, wander_target_with_room,
};
use crate::{
    interaction::{Flake, FoodPool, Stimuli},
    scape::sand_height,
    sdf::DecorSdf,
};

#[derive(Clone, Copy, PartialEq, Debug)]
enum State {
    Forage,
    Pause,
    Eat,
    Flee,
    Hidden,
}

struct Leg {
    root: Vec3,
    rest: Vec3,
    foot: Vec3,
    from: Vec3,
    to: Vec3,
    /// Step progress, >= 1 when planted.
    t: f32,
    group: usize,
    segs: [Entity; 2],
    lengths: [f32; 2],
    /// Last knee position (world).
    knee: Vec3,
    /// Smoothed bending direction of the knee (world).
    pole: Vec3,
}

struct Arm {
    root: Vec3,
    side: f32,
    segs: [Entity; 2],
    hand: Entity,
    lengths: [f32; 2],
    hand_len: f32,
    /// Current hand position (body frame), smoothed.
    pos: Vec3,
    /// Last elbow and claw tip (world), for the physics test.
    elbow: Vec3,
    tip: Vec3,
}

#[derive(Component)]
pub struct Crab {
    body: Crawler,
    state: State,
    timer: f32,
    think: f32,
    target: Vec3,
    food: Option<Entity>,
    width: f32,
    /// +1: walking towards its right, -1: left.
    side: f32,
    speed: f32,
    alert: f32,
    pick: f32,
    legs: Vec<Leg>,
    arms: Vec<Arm>,
    logged: State,
    /// Moved onto open sand once the full distance field is known.
    placed: bool,
    /// Where it was when the progress check last ran, and how long ago.
    progress_at: Vec3,
    progress_timer: f32,
}

pub fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CritterMaterial>>,
) {
    let width = 0.05;
    // Red-claw crab: dark red-brown shell, bright red claws with pale tips.
    let shell = Vec3::new(0.34, 0.1, 0.06);
    let belly = Vec3::new(0.85, 0.62, 0.45);
    let leg_c = Vec3::new(0.5, 0.17, 0.08);
    let claw_c = Vec3::new(0.9, 0.1, 0.04);
    let params = CritterParams {
        kind: Vec4::new(0.0, width, 24.0, 3.0),
        color_a: lin(0.34, 0.1, 0.06),
        color_b: lin(0.75, 0.55, 0.35),
        color_c: lin(0.9, 0.7, 0.5),
        look: Vec4::new(0.0, 40.0, 0.32, 0.0),
    };
    let material = critter_material(&mut materials, params, StandardMaterial { reflectance: 0.5, ..default() });
    let body_mesh = meshes.add(mesh::crab_body(width, shell, belly));
    let seg_mesh = meshes.add(mesh::limb_segment(leg_c, leg_c * 1.2));
    let arm_mesh = meshes.add(mesh::limb_segment(claw_c, claw_c));
    let claw_mesh = meshes.add(mesh::claw(claw_c, Vec3::new(0.95, 0.9, 0.85)));

    let start = Vec3::new(0.24, sand_height(0.24, 0.13), 0.13);
    let hover = width * 0.2;
    let segment = |commands: &mut Commands, m: &Handle<Mesh>| {
        commands
            .spawn((
                Name::new("crab limb"),
                Mesh3d(m.clone()),
                MeshMaterial3d(material.clone()),
                MeshTag(0),
                Transform::from_translation(start),
            ))
            .id()
    };
    let mut legs = Vec::new();
    for side in [-1.0f32, 1.0] {
        for (i, z) in [-0.17f32, -0.02, 0.12, 0.25].into_iter().enumerate() {
            let root = Vec3::new(side * 0.4 * width, -0.02 * width, z * width);
            let spread = 0.95 - 0.06 * i as f32;
            let rest = Vec3::new(side * spread * width, -hover, (z * 1.9 + 0.03) * width);
            legs.push(Leg {
                root,
                rest,
                foot: start + rest,
                from: start + rest,
                to: start + rest,
                t: 1.0,
                group: (i + if side > 0.0 { 1 } else { 0 }) % 2,
                segs: [segment(&mut commands, &seg_mesh), segment(&mut commands, &seg_mesh)],
                lengths: [0.44 * width, 0.5 * width],
                knee: start,
                pole: Vec3::Y,
            });
        }
    }
    let mut arms = Vec::new();
    for side in [-1.0f32, 1.0] {
        let root = Vec3::new(side * 0.26 * width, -0.02 * width, -0.3 * width);
        let hand = commands
            .spawn((
                Name::new("crab claw"),
                Mesh3d(claw_mesh.clone()),
                MeshMaterial3d(material.clone()),
                MeshTag(0),
                Transform::from_translation(start),
            ))
            .id();
        arms.push(Arm {
            root,
            side,
            segs: [segment(&mut commands, &arm_mesh), segment(&mut commands, &arm_mesh)],
            hand,
            lengths: [0.28 * width, 0.24 * width],
            hand_len: 0.42 * width * if side > 0.0 { 1.15 } else { 0.9 },
            pos: Vec3::new(side * 0.15 * width, -0.05 * width, -0.55 * width),
            elbow: start,
            tip: start,
        });
    }
    commands.spawn((
        critter_bundle("crab", body_mesh, material.clone(), Transform::from_translation(start)),
        Crab {
            // Sand and gentle slopes only, body kept fairly level: the legs
            // find the ground reliably and the crab never ends up on its side.
            body: Crawler::new(start, Vec3::new(-0.3, 0.0, 1.0), hover, 0.75, false).swimmer(0.5),
            state: State::Pause,
            timer: 2.0,
            think: 0.0,
            target: start,
            food: None,
            width,
            side: 1.0,
            speed: 0.0,
            alert: 0.0,
            pick: 0.0,
            legs,
            arms,
            logged: State::Pause,
            placed: false,
            progress_at: start,
            progress_timer: 0.0,
        },
    ));
}

/// The crab's mind: 4 decisions per second.
#[allow(clippy::too_many_arguments)]
pub fn think(
    time: Res<Time>,
    sdf: Res<DecorSdf>,
    stimuli: Res<Stimuli>,
    threats: Res<Threats>,
    spots: Res<HideSpots>,
    mut pool: ResMut<FoodPool>,
    mut puffs: ResMut<SandPuffs>,
    mut flakes: Query<(&mut Flake, &mut Visibility)>,
    mut crabs: Query<&mut Crab>,
) {
    let dt = time.delta_secs();
    let mut r = rng();
    if !sdf.complete {
        return;
    }
    for mut c in &mut crabs {
        if !c.placed {
            // The spawn point may be under a rock (unknown before the scanned
            // meshes were baked): start on open sand, while the screen is black.
            let p = super::wander_target_with_room(&sdf, c.body.pos, 0.15, true, 0.04);
            c.body.pos = p;
            c.target = p;
            c.placed = true;
        }
        c.timer -= dt;
        c.think -= dt;
        if c.think > 0.0 {
            continue;
        }
        c.think = 0.25;
        let pos = c.body.pos;
        let before = c.state;
        log_transition(&mut c, before, pos);

        // Danger first.
        let threat = threats.near(&stimuli, pos, 0.12).or(threats.startle.then_some((0.0, Vec3::X)));
        if let Some((_, away)) = threat {
            c.alert = 1.0;
            match c.state {
                // Already safe (stays in longer) or on its way: carry on below.
                State::Hidden => c.timer = c.timer.max(8.0),
                State::Flee => {}
                _ => {
                    // The closest hiding place that isn't towards the danger.
                    let spot = spots
                        .spots
                        .iter()
                        .copied()
                        .min_by(|a, b| {
                            let score = |s: &Vec3| s.distance(pos) - 0.08 * (*s - pos).normalize_or_zero().dot(away);
                            score(a).total_cmp(&score(b))
                        })
                        .unwrap_or(pos + away * 0.2);
                    c.target = spot;
                    c.state = State::Flee;
                    c.food = None;
                    c.timer = 0.0;
                    puffs.requests.push((pos, 0.4));
                    continue;
                }
            }
        } else {
            c.alert = (c.alert - 0.25 * 0.3).max(0.0);
        }

        match c.state {
            State::Forage | State::Pause => {
                // Food lying nearby?
                if let Some(f) = stimuli
                    .food
                    .iter()
                    .filter(|f| f.resting && f.pos.distance(pos) < 0.35)
                    .min_by(|a, b| a.pos.distance(pos).total_cmp(&b.pos.distance(pos)))
                {
                    c.state = State::Eat;
                    c.food = Some(f.entity);
                    c.target = f.pos;
                } else if c.state == State::Pause && c.timer <= 0.0 {
                    c.state = State::Forage;
                    c.target = wander_target_with_room(&sdf, pos, 0.22, true, 0.035);
                    c.timer = 10.0;
                } else if c.state == State::Forage && (pos.distance(c.target) < 0.012 || c.timer <= 0.0) {
                    c.state = State::Pause;
                    c.timer = r.random_range(1.5..5.0);
                }
            }
            State::Eat => {
                let food = c.food.and_then(|e| stimuli.food.iter().find(|f| f.entity == e));
                match food {
                    None => {
                        c.state = State::Pause;
                        c.timer = 1.0;
                    }
                    Some(f) => {
                        c.target = f.pos;
                        if f.pos.distance(pos) < 0.02 {
                            if let Ok((mut flake, mut vis)) = flakes.get_mut(f.entity) {
                                pool.consume(f.entity, &mut flake, &mut vis);
                            }
                            c.state = State::Pause;
                            c.timer = 3.0;
                            c.food = None;
                        }
                    }
                }
            }
            State::Flee => {
                if pos.distance(c.target) < 0.015 || c.timer < -6.0 {
                    c.state = State::Hidden;
                    c.timer = r.random_range(10.0..25.0);
                    puffs.requests.push((pos, 0.6));
                }
            }
            State::Hidden => {
                if c.timer <= 0.0 {
                    c.state = State::Forage;
                    c.target = wander_target_with_room(&sdf, pos, 0.2, true, 0.035);
                    c.timer = 10.0;
                }
            }
        }
    }
}

/// Logs the previous tick's decision (`AQ_LOG_CRITTERS`).
fn log_transition(c: &mut Crab, now: State, pos: Vec3) {
    if super::log_critters() && c.logged != now {
        info!("crab: {:?} -> {:?} at ({:.2}, {:.2}, {:.2})", c.logged, now, pos.x, pos.y, pos.z);
    }
    c.logged = now;
}

/// Body motion: sideways walk along the surface, at the display rate.
pub fn walk(time: Res<Time>, sdf: Res<DecorSdf>, mut crabs: Query<(&mut Crab, &mut Transform, &mut MeshTag)>) {
    let dt = time.delta_secs().min(0.05);
    if !sdf.complete {
        return;
    }
    for (mut c, mut tr, mut tag) in &mut crabs {
        let (speed, accel) = match c.state {
            State::Flee => (0.085, 0.5),
            State::Forage | State::Eat => (0.025, 0.1),
            _ => (0.0, 0.2),
        };
        let to = c.target - c.body.pos;
        let dist = to.length();
        let up = c.body.up;
        let dir = (to - up * to.dot(up)).normalize_or_zero();
        let arrive = (dist / 0.03).min(1.0);
        // Crabs run sideways. The body turns (at most a quarter turn) so that
        // a flank points at the goal, then keeps that heading: no re-aiming
        // near the goal, where the direction to it swings around.
        let right = c.body.facing.cross(up).normalize_or(Vec3::X);
        let mut face = c.body.facing;
        if dist > 0.03 && dir != Vec3::ZERO {
            let lateral = right * c.side;
            if lateral.dot(dir) < 0.85 {
                c.side = if right.dot(dir) >= 0.0 { 1.0 } else { -1.0 };
                face = if c.side > 0.0 { up.cross(dir) } else { dir.cross(up) };
            }
        }
        // Move along the flank, bending towards the goal as the body turns.
        let lateral = right * c.side;
        let heading = if dir == Vec3::ZERO { Vec3::ZERO } else { (lateral * 0.6 + dir * 0.4).normalize_or(dir) };
        let desired = heading * speed * arrive;
        let turn = if c.state == State::Flee { 3.0 } else { 1.2 };
        let width = c.width;
        let body = &mut c.body;
        let mut moved = body.step(&sdf, desired, face, accel, turn, dt);
        // The contact point follows the ground, but the carapace is 5 cm wide:
        // walking along a rock would push it into the stone. Slide along it.
        if keep_shell_out(&sdf, body, width) && body.vel.dot(heading) < 0.2 * speed * arrive {
            moved = false;
        }
        if !moved && c.state != State::Hidden {
            // Blocked (glass, surface): give up this goal.
            c.timer = c.timer.min(0.0);
        }
        c.speed = c.body.vel.length();
        // Wedged between rocks (trying to walk, going nowhere): back out,
        // away from the rock.
        c.progress_timer += dt;
        if c.progress_timer > 1.5 {
            let stuck = speed > 0.0 && c.body.pos.distance(c.progress_at) < 0.005 && dist > 0.015;
            if stuck {
                c.target = away_from_rocks(&sdf, c.body.pos, 0.07);
            }
            c.progress_at = c.body.pos;
            c.progress_timer = 0.0;
        }
        // Hidden: pressed down low.
        let tuck = if c.state == State::Hidden { 0.45 } else { 0.0 };
        c.body.sink += (tuck * c.body.hover - c.body.sink) * (1.0 - (-dt * 3.0).exp());
        *tr = c.body.transform(1.0);
        let packed = pack_tag(0.0, 0.0, 0.0, c.alert);
        if tag.0 != packed {
            tag.0 = packed;
        }
    }
}

/// Read-only view for the physics test (`AQ_PHYSICS`).
pub struct CrabProbe {
    pub state: &'static str,
    pub contact: Vec3,
    pub up: Vec3,
    pub facing: Vec3,
    pub speed: f32,
    /// Body centre (what must stay out of the rocks).
    pub centre: Vec3,
    /// Per leg: foot, planted, hip, reach.
    pub feet: Vec<(Vec3, bool, Vec3, f32)>,
    /// Points of the carapace outline and the eyes (world).
    pub shell: Vec<Vec3>,
    /// Knees, elbows and claw tips (world).
    pub knees: Vec<Vec3>,
}

impl Crab {
    pub fn probe(&self, body: &Transform) -> CrabProbe {
        CrabProbe {
            state: match self.state {
                State::Forage => "forage",
                State::Pause => "pause",
                State::Eat => "eat",
                State::Flee => "flee",
                State::Hidden => "hidden",
            },
            contact: self.body.pos,
            up: self.body.up,
            facing: self.body.facing,
            speed: self.body.vel.length(),
            centre: body.translation,
            feet: self
                .legs
                .iter()
                .map(|l| (l.foot, l.t >= 1.0, body.transform_point(l.root), l.lengths[0] + l.lengths[1]))
                .collect(),
            shell: shell_points(self.width).map(|p| body.transform_point(p)).to_vec(),
            knees: self.legs.iter().map(|l| l.knee).chain(self.arms.iter().flat_map(|a| [a.elbow, a.tip])).collect(),
        }
    }
}

/// The carapace outline (12 points at mid-height on its rounded-square rim)
/// and the two eyes, in the body frame (unit scale).
fn shell_points(w: f32) -> [Vec3; 14] {
    let mut out = [Vec3::ZERO; 14];
    for (i, o) in out.iter_mut().take(12).enumerate() {
        let a = i as f32 / 12.0 * std::f32::consts::TAU;
        let (c, s) = (a.cos(), a.sin());
        // Superellipse (power 4), like the mesh.
        let k = (c.abs().powi(4) + s.abs().powi(4)).powf(-0.25);
        *o = Vec3::new(c * k * 0.5 * w, 0.0, s * k * 0.4 * w);
    }
    out[12] = Vec3::new(-0.18 * w, 0.27 * w, -0.4 * w);
    out[13] = Vec3::new(0.18 * w, 0.27 * w, -0.4 * w);
    out
}

/// Pushes the body out of the rocks the carapace overlaps (sideways only: the
/// sand below is the contact point's business), and removes the velocity into
/// them. Returns whether it had to.
fn keep_shell_out(sdf: &DecorSdf, body: &mut Crawler, w: f32) -> bool {
    const MARGIN: f32 = 0.002;
    let mut hit = false;
    for _ in 0..3 {
        let tr = body.transform(1.0);
        // The deepest point of the outline that is against a rock face.
        let mut worst: Option<(f32, Vec3)> = None;
        for p in shell_points(w) {
            let q = tr.transform_point(p);
            let (d, n) = sdf.sample(q);
            let side = Vec3::new(n.x, 0.0, n.z);
            if d < MARGIN && q.y > sand_height(q.x, q.z) + 0.004 && side.length() > 0.3 {
                let depth = MARGIN - d;
                if worst.is_none_or(|(w, _)| depth > w) {
                    worst = Some((depth, side.normalize()));
                }
            }
        }
        let Some((depth, out)) = worst else {
            break;
        };
        hit = true;
        body.pos = sdf.drop_to_ground(body.pos + out * depth.min(0.006));
        let into = body.vel.dot(out);
        if into < 0.0 {
            body.vel -= out * into;
        }
    }
    hit
}

/// Knee bending direction: up and out like a real crab, unless that puts the
/// knee in a rock (under an overhang, against a boulder). Then the knee turns
/// around the hip-foot axis, as little as needed, to where there is room (or
/// to the roomiest place).
fn knee_pole(sdf: &DecorSdf, root: Vec3, foot: Vec3, lengths: [f32; 2], up: Vec3, out: Vec3) -> Vec3 {
    const ROOM: f32 = 0.004;
    let preferred = up * 0.7 + out;
    let axis = (foot - root).normalize_or(Vec3::NEG_Y);
    let u = (preferred - axis * preferred.dot(axis)).normalize_or(up);
    let v = axis.cross(u);
    let mut best = (f32::MIN, preferred);
    for k in [0.0f32, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0, 5.0, -5.0, 6.0] {
        let a = k * std::f32::consts::FRAC_PI_6;
        let pole = u * a.cos() + v * a.sin();
        let (knee, _) = ik(root, foot, lengths[0], lengths[1], pole);
        let room = sdf.distance(knee).min(sdf.distance((root + knee) * 0.5)).min(sdf.distance((knee + foot) * 0.5));
        if room > ROOM {
            return pole;
        }
        if room > best.0 {
            best = (room, pole);
        }
    }
    best.1
}

/// Two-bone IK: knee position for a limb from `root` to `target`.
fn ik(root: Vec3, target: Vec3, l1: f32, l2: f32, pole: Vec3) -> (Vec3, Vec3) {
    let to = target - root;
    let d = to.length().clamp((l1 - l2).abs() + 1e-4, l1 + l2 - 1e-4);
    let dir = to.normalize_or(Vec3::Z);
    let end = root + dir * d;
    let a = (l1 * l1 - l2 * l2 + d * d) / (2.0 * d);
    let h = (l1 * l1 - a * a).max(0.0).sqrt();
    let bend = (pole - dir * pole.dot(dir)).normalize_or(Vec3::Y);
    (root + dir * a + bend * h, end)
}

fn segment(a: Vec3, b: Vec3, radius: f32) -> Transform {
    let d = b - a;
    Transform {
        translation: a,
        rotation: Quat::from_rotation_arc(Vec3::Z, d.normalize_or(Vec3::Z)),
        scale: Vec3::new(radius, radius, d.length().max(1e-4)),
    }
}

/// Legs and claws: planted feet, alternating steps, IK.
pub fn legs(
    time: Res<Time>,
    sdf: Res<DecorSdf>,
    mut crabs: Query<(&mut Crab, &Transform)>,
    mut parts: Query<&mut Transform, Without<Crab>>,
) {
    let dt = time.delta_secs().min(0.05);
    let t_now = time.elapsed_secs();
    for (mut c, tr) in &mut crabs {
        let c = &mut *c;
        let w = c.width;
        let body = *tr;
        let up = c.body.up;
        let speed = c.speed;
        let vel = c.body.vel;
        // Faster walking: quicker, longer steps.
        let step_time = (0.16 - speed * 0.8).clamp(0.06, 0.16);
        let stride = w * (0.14 + speed * 1.0).min(0.3);
        // Feet land on the ground below (never on the ceiling of a hideout).
        let ground = |p: Vec3| sdf.drop_to_ground(p);
        let stepping = [0usize, 1].map(|g| c.legs.iter().any(|l| l.group == g && l.t < 1.0));
        let mut started = [false; 2];
        for leg in c.legs.iter_mut() {
            let rest_world = body.transform_point(leg.rest);
            let reach = leg.lengths[0] + leg.lengths[1];
            let hip = body.transform_point(leg.root);
            if leg.t >= 1.0 {
                let other = 1 - leg.group;
                let far = leg.foot.distance(rest_world) > stride;
                // A foot out of reach steps at once, whatever the gait says.
                let overstretched = hip.distance(leg.foot) > reach * 0.95;
                if (far && !stepping[other] && !started[other]) || overstretched {
                    let to = ground(rest_world + vel * step_time * 1.2);
                    if hip.distance(leg.foot) > reach * 1.5 {
                        // Way off (teleported, hidden): just put it down.
                        leg.foot = to;
                    } else {
                        leg.from = leg.foot;
                        leg.to = to;
                        leg.t = 0.0;
                        started[leg.group] = true;
                    }
                }
            }
            if leg.t < 1.0 {
                leg.t = (leg.t + dt / step_time).min(1.0);
                let s = leg.t * leg.t * (3.0 - 2.0 * leg.t);
                leg.foot = leg.from.lerp(leg.to, s) + up * ((leg.t * std::f32::consts::PI).sin() * w * 0.18);
                // A foot in the air goes over a rock edge, not through it.
                let (d, n) = sdf.sample(leg.foot);
                if d < 0.001 && leg.t < 1.0 {
                    leg.foot += n * (0.001 - d);
                }
            }
            let root = body.transform_point(leg.root);
            let outward = (rest_world - root).normalize_or(Vec3::X);
            let pole = knee_pole(&sdf, root, leg.foot, leg.lengths, up, outward).normalize_or(up);
            leg.pole = leg.pole.lerp(pole, 1.0 - (-dt * 14.0).exp()).normalize_or(up);
            // The eased knee would cut through the rock: straight to the free side.
            let (eased, _) = ik(root, leg.foot, leg.lengths[0], leg.lengths[1], leg.pole);
            if sdf.distance(eased) < 0.003 {
                leg.pole = pole;
            }
            let (knee, end) = ik(root, leg.foot, leg.lengths[0], leg.lengths[1], leg.pole);
            leg.knee = knee;
            if let Ok(mut t) = parts.get_mut(leg.segs[0]) {
                *t = segment(root, knee, w * 0.058);
            }
            if let Ok(mut t) = parts.get_mut(leg.segs[1]) {
                *t = segment(knee, end, w * 0.044);
            }
        }

        // Claws: folded in front; picking at the ground when idle; raised when alarmed.
        c.pick += dt * if matches!(c.state, State::Pause | State::Eat) { 2.2 } else { 0.0 };
        for arm in c.arms.iter_mut() {
            let s = arm.side;
            let rest = Vec3::new(s * 0.16 * w, -0.04 * w, -0.55 * w);
            let goal = if c.alert > 0.5 || c.state == State::Flee {
                // Raised in threat... unless there is a rock above (hideout).
                let raised = Vec3::new(s * 0.42 * w, 0.32 * w, -0.5 * w);
                let room = sdf.distance(body.transform_point(raised));
                rest.lerp(raised, ((room - 0.004) / 0.008).clamp(0.0, 1.0))
            } else if matches!(c.state, State::Pause | State::Eat) {
                // Alternate: one hand to the ground, the other to the mouth.
                let phase = (c.pick + if s > 0.0 { std::f32::consts::PI } else { 0.0 }).sin() * 0.5 + 0.5;
                let ground = Vec3::new(s * 0.2 * w, -c.body.hover + 0.02 * w, -0.72 * w);
                let mouth = Vec3::new(s * 0.05 * w, -0.02 * w, -0.42 * w);
                ground.lerp(mouth, phase)
            } else {
                rest + Vec3::Y * ((t_now * 1.3 + s).sin() * 0.02 * w)
            };
            // Never into a rock (picking at the ground at the foot of a boulder,
            // hiding under one): the wrist and the tip of the claw keep clear.
            let mut goal = goal;
            for _ in 0..2 {
                let (wrist, tip) = (body.transform_point(goal), body.transform_point(goal + Vec3::NEG_Z * arm.hand_len));
                let ((dw, nw), (dt_, nt)) = (sdf.sample(wrist), sdf.sample(tip));
                let (d, n) = if dw < dt_ { (dw, nw) } else { (dt_, nt) };
                if d >= 0.004 {
                    break;
                }
                goal += body.rotation.inverse() * n * (0.004 - d);
            }
            arm.pos = arm.pos.lerp(goal, 1.0 - (-dt * 8.0).exp());
            // The smoothed hand lags behind its goal (a claw raised while the
            // crab dives under a rock): keep the arm and the claw itself out of
            // the stone, checked where they are actually drawn.
            let root = body.transform_point(arm.root);
            let pole = up * 0.6 + body.rotation * Vec3::new(s, 0.0, 0.3);
            let fwd = body.rotation * Vec3::NEG_Z;
            let (mut elbow, mut end, mut dir) = (root, root, fwd);
            for _ in 0..3 {
                let wrist = body.transform_point(arm.pos);
                (elbow, end) = ik(root, wrist, arm.lengths[0], arm.lengths[1], pole);
                dir = ((end - elbow).normalize_or(fwd) + fwd * 0.8).normalize_or(fwd);
                let tip = end + dir * arm.hand_len * 0.8;
                let (d, n) = [elbow, end, tip]
                    .map(|q| sdf.sample(q))
                    .into_iter()
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .unwrap();
                if d >= 0.003 {
                    break;
                }
                arm.pos += body.rotation.inverse() * n * (0.003 - d);
            }
            arm.elbow = elbow;
            if let Ok(mut t) = parts.get_mut(arm.segs[0]) {
                *t = segment(root, elbow, w * 0.07);
            }
            if let Ok(mut t) = parts.get_mut(arm.segs[1]) {
                *t = segment(elbow, end, w * 0.06);
            }
            if let Ok(mut t) = parts.get_mut(arm.hand) {
                arm.tip = end + dir * arm.hand_len * 0.8;
                *t = Transform {
                    translation: end,
                    rotation: Quat::from_rotation_arc(Vec3::Z, dir),
                    scale: Vec3::splat(arm.hand_len),
                };
            }
        }
    }
}

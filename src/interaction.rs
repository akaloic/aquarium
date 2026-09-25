//! Mouse interaction.
//!
//! * The cursor is a "finger" poking into the water: the part of its ray inside
//!   the tank, up to the first thing it touches (rock, sand, back glass). Fish
//!   close to it dart away (panic spreads through the school, then calms down in
//!   2-3 s) and the suspended specks swirl around it.
//! * A left click drops a pinch of food flakes above the pointed spot: rings
//!   spread on the surface (and through the caustics on the floor), the flakes
//!   float, then sink like falling leaves and settle on the decor; the fish rush
//!   to eat them, the bottom dwellers pick up what reaches the ground.
//! * The depth of field follows what you point at (or the nearest fish).

use std::f32::consts::TAU;

use bevy::{
    light::NotShadowCaster,
    post_process::dof::DepthOfField,
    prelude::*,
    window::PrimaryWindow,
};
use rand::{RngExt, SeedableRng, rng, rngs::StdRng};

use crate::{
    config::AppConfig,
    fish::{BubbleRequests, Fish, FishSystems},
    meshgen::MeshBuilder,
    sdf::DecorSdf,
    tank::{HALF_D, HALF_W, WATER_Y},
    water::{Cookie, ParticleMaterial, WaterSurfaceMaterial, WaterUndersideMaterial},
};

/// What the animals can sense this frame.
#[derive(Resource, Default)]
pub struct Stimuli {
    pub cursor: Option<CursorProbe>,
    pub food: Vec<FoodItem>,
}

#[derive(Clone, Copy)]
pub struct FoodItem {
    pub entity: Entity,
    pub pos: Vec3,
    /// Lying on the sand or a rock (for the bottom dwellers).
    pub resting: bool,
}

/// The part of the cursor ray inside the water.
pub struct CursorProbe {
    pub from: Vec3,
    pub to: Vec3,
    /// Velocity of the finger (m/s) and its magnitude.
    pub velocity: Vec3,
    pub speed: f32,
}

/// Where the cursor is when the window can't tell: the wallpaper mode (the
/// window ignores the mouse, the position is read from the system) and the
/// headless autopilot. Window logical pixels, or viewport pixels without window.
#[derive(Resource, Default)]
pub struct ExternalCursor(pub Option<Vec2>);

pub struct InteractionPlugin;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum InteractionSystems {
    Sense,
    React,
}

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Stimuli>()
            .init_resource::<CursorState>()
            .init_resource::<Drops>()
            .init_resource::<FoodPool>()
            .init_resource::<ExternalCursor>()
            .configure_sets(
                Update,
                (
                    InteractionSystems::Sense.before(FishSystems),
                    InteractionSystems::React.after(FishSystems),
                ),
            )
            .add_systems(Startup, spawn_food_pool)
            .add_systems(
                Update,
                (sense_cursor, drop_food, update_food)
                    .chain()
                    .in_set(InteractionSystems::Sense),
            )
            .add_systems(
                Update,
                (eat_food, push_ripples, stir_particles, autofocus).in_set(InteractionSystems::React),
            );
    }
}

// ---------------------------------------------------------------------------
// Cursor
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct CursorState {
    last_pos: Option<Vec2>,
    /// Real time of the last cursor motion.
    last_move: f64,
    prev_mid: Option<Vec3>,
    velocity: Vec3,
    /// Stirring of the particles (0..1), decays after the cursor stops.
    stir: f32,
    stir_a: Vec3,
    stir_b: Vec3,
    stir_dir: Vec3,
    /// Point on the ray closest to the tank centre (where food is dropped).
    mid: Option<Vec3>,
    pub active: bool,
}

/// Slab test of a ray against the water box: (t_enter, t_exit).
fn water_box_hit(o: Vec3, d: Vec3) -> Option<(f32, f32)> {
    let lo = Vec3::new(-HALF_W, 0.0, -HALF_D);
    let hi = Vec3::new(HALF_W, WATER_Y, HALF_D);
    let inv = d.recip();
    let t1 = (lo - o) * inv;
    let t2 = (hi - o) * inv;
    let tmin = t1.min(t2).max_element().max(0.0);
    let tmax = t1.max(t2).min_element();
    (tmax > tmin).then_some((tmin, tmax))
}

fn sense_cursor(
    time: Res<Time<Real>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    external: Res<ExternalCursor>,
    buttons: Res<ButtonInput<MouseButton>>,
    cams: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    sdf: Res<DecorSdf>,
    mut state: ResMut<CursorState>,
    mut stimuli: ResMut<Stimuli>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs().max(1e-4);
    stimuli.cursor = None;
    state.mid = None;
    let decay = (-dt / 0.9).exp();
    state.stir *= decay;

    let window = windows.single().ok();
    let Ok((camera, cam_gt)) = cams.single() else {
        return;
    };
    let Some(viewport) = camera.logical_viewport_size() else {
        return;
    };
    let pos = external.0.or_else(|| window.and_then(|w| w.cursor_position()));
    let Some(pos) = pos else {
        state.last_pos = None;
        state.prev_mid = None;
        state.active = false;
        return;
    };
    if state.last_pos.is_none_or(|p| p.distance(pos) > 0.5) {
        state.last_move = now;
    }
    state.last_pos = Some(pos);
    // Fish get used to a cursor that stays still.
    state.active = now - state.last_move < 2.5 || buttons.pressed(MouseButton::Left);

    let size = window.map_or(viewport, |w| Vec2::new(w.width(), w.height()));
    let uv = pos / size.max(Vec2::ONE);
    let Ok(ray) = camera.viewport_to_world(cam_gt, uv * viewport) else {
        return;
    };
    let (o, d) = (ray.origin, *ray.direction);
    let Some((t0, t1)) = water_box_hit(o, d) else {
        state.prev_mid = None;
        return;
    };
    let from = o + d * t0;
    // First surface inside the water (skip the pane we came through).
    let start = from + d * 0.015;
    let len = (t1 - t0 - 0.015).max(0.0);
    let to = start + d * sdf.march(start, d, len).unwrap_or(len);

    // Motion of the finger around the middle of the tank: smooth, and immune to
    // the tip jumping from a near rock to the far glass.
    let centre = Vec3::new(0.0, WATER_Y * 0.5, 0.0);
    let mid = o + d * (centre - o).dot(d).clamp(t0, t1);
    state.mid = Some(mid);
    let raw = state.prev_mid.map_or(Vec3::ZERO, |p| (mid - p) / dt);
    state.prev_mid = Some(mid);
    let k = 1.0 - (-dt * 12.0).exp();
    state.velocity = state.velocity.lerp(raw.clamp_length_max(3.0), k);
    let speed = state.velocity.length();

    if state.active {
        let stir = (speed / 0.3).min(1.0);
        if stir > state.stir {
            state.stir = stir;
        }
        state.stir_a = from;
        state.stir_b = to;
        state.stir_dir = state.velocity.normalize_or_zero();
        stimuli.cursor = Some(CursorProbe {
            from,
            to,
            velocity: state.velocity,
            speed,
        });
    }
}

// ---------------------------------------------------------------------------
// Rings on the surface
// ---------------------------------------------------------------------------

/// Surface rings: (x, z, start time, strength), round robin.
#[derive(Resource, Default)]
pub struct Drops {
    rings: [Vec4; 4],
    next: usize,
    dirty: bool,
}

impl Drops {
    pub fn add(&mut self, x: f32, z: f32, time: f32, strength: f32) {
        self.rings[self.next] = Vec4::new(x, z, time, strength);
        self.next = (self.next + 1) % self.rings.len();
        self.dirty = true;
    }
}

fn push_ripples(
    mut drops: ResMut<Drops>,
    mut surface: ResMut<Assets<WaterSurfaceMaterial>>,
    mut underside: ResMut<Assets<WaterUndersideMaterial>>,
    mut cookie: ResMut<Cookie>,
) {
    if !drops.dirty {
        return;
    }
    drops.dirty = false;
    for (_, m) in surface.iter_mut() {
        m.extension.drops = drops.rings;
    }
    for (_, m) in underside.iter_mut() {
        m.params.drops = drops.rings;
    }
    cookie.params.drops = drops.rings;
}

fn stir_particles(
    state: Res<CursorState>,
    mut was_on: Local<bool>,
    mut materials: ResMut<Assets<ParticleMaterial>>,
) {
    let on = state.stir > 0.002;
    if !on && !*was_on {
        return;
    }
    *was_on = on;
    let strength = if on { state.stir } else { 0.0 };
    for (_, m) in materials.iter_mut() {
        m.particles.cursor_a = state.stir_a.extend(strength);
        m.particles.cursor_b = state.stir_b.extend(0.035);
        m.particles.cursor_v = state.stir_dir.extend(0.0);
    }
}

// ---------------------------------------------------------------------------
// Food
// ---------------------------------------------------------------------------

const FLAKES: usize = 64;

#[derive(Clone, Copy, Default, PartialEq, Debug)]
enum FlakeState {
    #[default]
    Idle,
    Waiting,
    Floating,
    Sinking,
    Resting,
    Fading,
}

#[derive(Component, Default)]
pub struct Flake {
    state: FlakeState,
    t: f32,
    wait: f32,
    float_time: f32,
    vel: Vec3,
    sink: f32,
    spin_axis: Vec3,
    spin: f32,
    phase: f32,
    flutter: Vec3,
    size: f32,
}

impl Flake {
    /// (state, velocity) for the physics test; None when in the pool.
    pub fn probe(&self) -> Option<(&'static str, Vec3)> {
        let s = match self.state {
            FlakeState::Idle | FlakeState::Waiting => return None,
            FlakeState::Floating => "floating",
            FlakeState::Sinking => "sinking",
            FlakeState::Resting => "resting",
            FlakeState::Fading => "fading",
        };
        Some((s, self.vel))
    }

    pub fn edible(&self) -> bool {
        matches!(self.state, FlakeState::Floating | FlakeState::Sinking | FlakeState::Resting)
    }
    pub fn resting(&self) -> bool {
        self.state == FlakeState::Resting
    }
}

#[derive(Resource, Default)]
pub struct FoodPool {
    free: Vec<Entity>,
}

impl FoodPool {
    /// Removes an eaten flake from the water.
    pub fn consume(&mut self, e: Entity, flake: &mut Flake, vis: &mut Visibility) {
        if flake.state != FlakeState::Idle {
            flake.state = FlakeState::Idle;
            *vis = Visibility::Hidden;
            self.free.push(e);
        }
    }
}

/// An irregular flake: a thin polygon, slightly cupped.
fn flake_mesh(rng: &mut StdRng) -> Mesh {
    let mut b = MeshBuilder::default();
    let n = rng.random_range(6..10);
    let c = b.vertex(Vec3::new(0.0, 0.08, 0.0), Vec3::Y, Vec2::splat(0.5));
    let radii: Vec<f32> = (0..n).map(|_| rng.random_range(0.55..1.0)).collect();
    for i in 0..n {
        let a = i as f32 / n as f32 * TAU;
        let r = radii[i];
        b.vertex(Vec3::new(a.cos() * r, 0.0, a.sin() * r), Vec3::Y, Vec2::new(0.5 + a.cos() * 0.5, 0.5 + a.sin() * 0.5));
    }
    for i in 0..n {
        b.tri(c, c + 1 + i as u32, c + 1 + ((i + 1) % n) as u32);
    }
    b.compute_smooth_normals();
    b.orient_to_normals();
    b.build()
}

fn spawn_food_pool(
    mut commands: Commands,
    mut pool: ResMut<FoodPool>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut rng = StdRng::seed_from_u64(0xF00D);
    let shapes: Vec<Handle<Mesh>> = (0..4).map(|_| meshes.add(flake_mesh(&mut rng))).collect();
    // Colourful flake food: red, orange, yellow, green, brown.
    let colors = [
        Color::srgb(0.78, 0.12, 0.06),
        Color::srgb(0.92, 0.45, 0.08),
        Color::srgb(0.95, 0.78, 0.25),
        Color::srgb(0.35, 0.55, 0.12),
        Color::srgb(0.45, 0.28, 0.12),
    ];
    let mats: Vec<Handle<StandardMaterial>> = colors
        .iter()
        .map(|&c| {
            materials.add(StandardMaterial {
                base_color: c,
                perceptual_roughness: 0.75,
                reflectance: 0.3,
                diffuse_transmission: 0.35,
                double_sided: true,
                cull_mode: None,
                ..default()
            })
        })
        .collect();
    pool.free.reserve(FLAKES);
    for i in 0..FLAKES {
        // One of each look stays visible under the sand for a few seconds so its
        // pipeline is compiled before the first click.
        let warm = i < mats.len();
        let e = commands
            .spawn((
                Name::new("food flake"),
                Mesh3d(shapes[i % shapes.len()].clone()),
                MeshMaterial3d(mats[i % mats.len()].clone()),
                Transform::from_xyz(0.0, if warm { -0.03 } else { -1.0 }, 0.1).with_scale(Vec3::splat(0.003)),
                if warm { Visibility::Visible } else { Visibility::Hidden },
                Flake::default(),
                NotShadowCaster,
            ))
            .id();
        pool.free.push(e);
    }
}

fn drop_food(
    config: Res<AppConfig>,
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    state: Res<CursorState>,
    mut drops: ResMut<Drops>,
    mut pool: ResMut<FoodPool>,
    mut bubbles: ResMut<BubbleRequests>,
    mut flakes: Query<(&mut Flake, &mut Transform, &mut Visibility)>,
) {
    if config.wallpaper || !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(mid) = state.mid else {
        return;
    };
    let x = mid.x.clamp(-HALF_W + 0.05, HALF_W - 0.05);
    let z = mid.z.clamp(-HALF_D + 0.05, HALF_D - 0.05);
    let now = time.elapsed_secs_wrapped();
    drops.add(x, z, now, 0.0012);
    let mut rng = rng();
    for k in 0..rng.random_range(10..15) {
        let Some(e) = pool.free.pop() else {
            break;
        };
        let Ok((mut f, mut tr, mut vis)) = flakes.get_mut(e) else {
            continue;
        };
        let a = rng.random_range(0.0..TAU);
        let r = rng.random_range(0.0..0.015f32).sqrt() * 0.12;
        let out = Vec3::new(a.cos(), 0.0, a.sin());
        *f = Flake {
            state: FlakeState::Waiting,
            t: 0.0,
            wait: k as f32 * 0.025 + rng.random_range(0.0..0.2),
            float_time: rng.random_range(0.3..2.2),
            vel: out * rng.random_range(0.004..0.02),
            sink: rng.random_range(0.010..0.022),
            spin_axis: Vec3::new(rng.random_range(-1.0..1.0), rng.random_range(-0.3..0.3), rng.random_range(-1.0..1.0))
                .normalize_or(Vec3::X),
            spin: rng.random_range(1.5..5.0) * if rng.random_bool(0.5) { 1.0 } else { -1.0 },
            phase: rng.random_range(0.0..TAU),
            flutter: Vec3::new(rng.random_range(-1.0..1.0), 0.0, rng.random_range(-1.0..1.0)).normalize_or(Vec3::X),
            size: rng.random_range(0.0026..0.0042),
        };
        *tr = Transform::from_xyz(x + out.x * r, WATER_Y - 0.0012, z + out.z * r)
            .with_rotation(Quat::from_rotation_y(rng.random_range(0.0..TAU)))
            .with_scale(Vec3::splat(f.size));
        *vis = Visibility::Hidden;
        if k < 2 {
            bubbles.0.push((Vec3::new(x, WATER_Y - 0.006, z), 0.0008));
        }
    }
}

fn update_food(
    time: Res<Time>,
    sdf: Res<DecorSdf>,
    mut pool: ResMut<FoodPool>,
    mut stimuli: ResMut<Stimuli>,
    mut flakes: Query<(Entity, &mut Flake, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs().min(0.05);
    let t_now = time.elapsed_secs();
    stimuli.food.clear();
    for (e, mut f, mut tr, mut vis) in &mut flakes {
        match f.state {
            FlakeState::Idle => {
                // Warm-up flakes leave once the pipelines are compiled.
                if t_now > 5.0 && *vis == Visibility::Visible && tr.translation.y < 0.0 {
                    *vis = Visibility::Hidden;
                }
                continue;
            }
            FlakeState::Waiting => {
                f.wait -= dt;
                if f.wait <= 0.0 {
                    f.state = FlakeState::Floating;
                    f.t = 0.0;
                    *vis = Visibility::Visible;
                }
                continue;
            }
            FlakeState::Floating => {
                f.t += dt;
                // Spreads out on the surface film, bobbing on the ripples.
                let v = f.vel;
                tr.translation += Vec3::new(v.x, 0.0, v.z) * dt;
                f.vel *= (-dt * 1.2).exp();
                tr.translation.y = WATER_Y - 0.0012 + 0.0004 * (t_now * 5.0 + f.phase).sin();
                if f.t > f.float_time {
                    f.state = FlakeState::Sinking;
                    f.t = 0.0;
                    f.vel = Vec3::new(f.vel.x, -0.002, f.vel.z);
                }
            }
            FlakeState::Sinking => {
                f.t += dt;
                // Falling leaf: side-to-side glide, rocking, slow tumbling.
                let swing = (f.t * 2.6 + f.phase).sin();
                let target = Vec3::new(0.0, -f.sink * (0.75 + 0.25 * swing.abs()), 0.0) + f.flutter * (0.012 * swing);
                let k = 1.0 - (-dt * 3.0).exp();
                f.vel = f.vel.lerp(target, k);
                // The flutter plane slowly turns.
                f.flutter = Quat::from_rotation_y(dt * 0.4) * f.flutter;
                let rock = Quat::from_axis_angle(f.flutter.cross(Vec3::Y).normalize_or(Vec3::X), swing * 0.5 * dt * 2.6);
                tr.rotation = (Quat::from_axis_angle(f.spin_axis, f.spin * dt * 0.35) * rock * tr.rotation).normalize();
                let mut p = tr.translation + f.vel * dt;
                let (d, n) = sdf.sample(p);
                if d < 0.0015 {
                    if n.y > 0.45 {
                        // Settles on the sand or on a rock, lying flat.
                        let (q, n) = sdf.project(p, 0.0012);
                        p = q;
                        let yaw = Quat::from_rotation_y(f.phase * 3.0);
                        tr.rotation = Quat::from_rotation_arc(Vec3::Y, n) * yaw;
                        f.state = FlakeState::Resting;
                        f.t = 0.0;
                    } else {
                        // Slides down glass and steep rock faces.
                        p += n * (0.0015 - d);
                        let into = f.vel.dot(n).min(0.0);
                        f.vel -= n * into;
                    }
                }
                tr.translation = p;
            }
            FlakeState::Resting => {
                f.t += dt;
                if f.t > 40.0 {
                    f.state = FlakeState::Fading;
                    f.t = 0.0;
                }
            }
            FlakeState::Fading => {
                // Dissolves.
                f.t += dt;
                tr.scale = Vec3::splat(f.size * (1.0 - f.t / 2.5).max(0.0));
                if f.t > 2.5 {
                    pool.consume(e, &mut f, &mut vis);
                    continue;
                }
            }
        }
        if f.edible() {
            stimuli.food.push(FoodItem {
                entity: e,
                pos: tr.translation,
                resting: f.resting(),
            });
        }
    }
}

/// Fish snap the flakes that drift past their mouth.
fn eat_food(
    mut pool: ResMut<FoodPool>,
    mut bubbles: ResMut<BubbleRequests>,
    mut fish: Query<(&mut Fish, &Transform)>,
    mut flakes: Query<(Entity, &mut Flake, &Transform, &mut Visibility), Without<Fish>>,
) {
    let mut rng = rng();
    for (e, mut flake, ftr, mut vis) in &mut flakes {
        if !matches!(flake.state, FlakeState::Floating | FlakeState::Sinking) {
            continue;
        }
        let p = ftr.translation;
        for (mut f, tr) in &mut fish {
            if f.satiety >= 1.0 {
                continue;
            }
            let mouth = tr.translation + tr.forward() * (0.45 * f.length);
            if mouth.distance(p) < f.length * 0.3 + 0.006 {
                f.satiety += 0.3;
                f.gulp = 1.0;
                if rng.random_bool(0.25) {
                    bubbles.0.push((mouth, 0.0007));
                }
                pool.consume(e, &mut flake, &mut vis);
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Depth of field: rack focus on what you point at, else the nearest fish
// ---------------------------------------------------------------------------

fn autofocus(
    time: Res<Time<Real>>,
    stimuli: Res<Stimuli>,
    fish: Query<&GlobalTransform, With<Fish>>,
    mut cams: Query<(&GlobalTransform, &mut DepthOfField), With<Camera3d>>,
) {
    let Ok((cam, mut dof)) = cams.single_mut() else {
        return;
    };
    let eye = cam.translation();
    let fwd = cam.forward().as_vec3();
    let mut target = None;
    if let Some(c) = &stimuli.cursor {
        // The fish closest to the finger, else the surface it touches.
        let seg = c.to - c.from;
        let mut best = 0.06f32;
        for gt in &fish {
            let p = gt.translation();
            let s = ((p - c.from).dot(seg) / seg.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let d = p.distance(c.from + seg * s);
            if d < best {
                best = d;
                target = Some(p.distance(eye));
            }
        }
        target = target.or(Some(c.to.distance(eye)));
    }
    let target = target.unwrap_or_else(|| {
        // Nearest fish close to the centre of the view.
        fish.iter()
            .map(|gt| gt.translation() - eye)
            .filter(|v| v.normalize_or_zero().dot(fwd) > 0.96)
            .map(|v| v.length())
            .min_by(f32::total_cmp)
            .unwrap_or_else(|| (Vec3::new(0.0, WATER_Y * 0.45, 0.0) - eye).length())
    });
    let k = 1.0 - (-time.delta_secs() * 2.5).exp();
    dof.focal_distance += (target - dof.focal_distance) * k;
}

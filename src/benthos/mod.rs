//! Bottom dwellers: a crab, flatfish and starfish.
//!
//! They live *on* the decor: every step is projected onto the distance field of
//! the static scene (`sdf.rs`), so they walk over sand, rocks and — for the
//! starfish — the glass. Their minds tick at 4 Hz (choose a goal, notice a
//! threat, spot food); the motion itself (body, legs, fins) is integrated at the
//! display rate, so it stays smooth. They react to the cursor, to big fish
//! passing by, to food lying on the ground, and to the wallpaper waking up.

mod crab;
mod critters;
mod mesh;

pub use crab::Crab;
pub use critters::{Glider, Star};

use std::f32::consts::TAU;

use bevy::{
    light::NotShadowCaster,
    mesh::{MeshTag, MeshVertexBufferLayoutRef},
    pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline},
    prelude::*,
    render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError},
    shader::ShaderRef,
};
use rand::{RngExt, rng};

use crate::{
    fish::{Fish, FishSystems},
    interaction::{InteractionSystems, Stimuli},
    power::Power,
    scape::sand_height,
    sdf::DecorSdf,
    tank::{HALF_D, HALF_W, WATER_Y},
};

pub struct BenthosPlugin;

impl Plugin for BenthosPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<CritterMaterial>::default())
            .init_resource::<HideSpots>()
            .init_resource::<SandPuffs>()
            .init_resource::<Threats>()
            .add_systems(Startup, (load_shaders, spawn_puff_pool, crab::spawn, critters::spawn))
            .add_systems(
                Update,
                (
                    find_hide_spots,
                    sense_threats,
                    (crab::think, crab::walk, crab::legs).chain(),
                    critters::flatfish,
                    critters::starfish,
                    animate_puffs,
                    log_positions,
                )
                    .chain()
                    .after(FishSystems)
                    .after(InteractionSystems::Sense)
                    .before(InteractionSystems::React),
            )
            .add_systems(PostUpdate, follow_cam.before(bevy::transform::TransformSystems::Propagate));
    }
}

/// Debug: `AQ_CAM_FOLLOW=crab|flatfish|starfish` keeps the camera on one of them.
fn follow_cam(
    critters: Query<(&Name, &Transform), (With<MeshTag>, Without<Camera3d>)>,
    mut cams: Query<&mut Transform, With<Camera3d>>,
) {
    let Ok(who) = std::env::var("AQ_CAM_FOLLOW") else {
        return;
    };
    let Some((_, t)) = critters.iter().find(|(n, _)| n.as_str() == who) else {
        return;
    };
    if let Ok(mut cam) = cams.single_mut() {
        let at = t.translation;
        *cam = Transform::from_translation(at + Vec3::new(0.06, 0.09, 0.22)).looking_at(at, Vec3::Y);
    }
}

#[derive(Resource)]
#[allow(dead_code)]
struct CritterShaders(Handle<Shader>);

fn load_shaders(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(CritterShaders(assets.load("shaders/critter_anim.wgsl")));
}

// ---------------------------------------------------------------------------
// Material
// ---------------------------------------------------------------------------

pub type CritterMaterial = ExtendedMaterial<StandardMaterial, CritterExt>;

#[derive(ShaderType, Clone, Copy, Debug, Default, Reflect)]
pub struct CritterParams {
    /// x kind, y size (m), z pattern scale, w seed.
    pub kind: Vec4,
    pub color_a: Vec4,
    pub color_b: Vec4,
    pub color_c: Vec4,
    /// x translucency, y water ambient (nits), z roughness, w wave amplitude.
    pub look: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct CritterExt {
    #[uniform(100)]
    pub critter: CritterParams,
}

impl MaterialExtension for CritterExt {
    fn vertex_shader() -> ShaderRef {
        "shaders/critter.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/critter.wgsl".into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/critter_prepass.wgsl".into()
    }
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Thin fins and discs: both sides.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

pub fn critter_material(
    materials: &mut Assets<CritterMaterial>,
    params: CritterParams,
    base: StandardMaterial,
) -> Handle<CritterMaterial> {
    materials.add(CritterMaterial {
        base: StandardMaterial {
            double_sided: true,
            cull_mode: None,
            ..base
        },
        extension: CritterExt { critter: params },
    })
}

pub fn lin(r: f32, g: f32, b: f32) -> Vec4 {
    let c = Color::srgb(r, g, b).to_linear();
    Vec4::new(c.red, c.green, c.blue, 1.0)
}

/// Packs the animation state for the shaders (see critter_anim.wgsl).
pub fn pack_tag(phase: f32, amplitude: f32, buried: f32, alert: f32) -> u32 {
    let p = ((phase.rem_euclid(TAU) / TAU) * 4096.0) as u32 & 0xfff;
    let a = ((amplitude.clamp(0.0, 1.0) * 63.0).round() as u32) & 0x3f;
    let b = ((buried.clamp(0.0, 1.0) * 63.0).round() as u32) & 0x3f;
    let t = ((alert.clamp(0.0, 1.0) * 255.0).round() as u32) & 0xff;
    p | (a << 12) | (b << 18) | (t << 24)
}

// ---------------------------------------------------------------------------
// Walking on the decor
// ---------------------------------------------------------------------------

/// A body that moves over the surface of the decor.
#[derive(Clone, Copy)]
pub struct Crawler {
    /// Contact point on the surface.
    pub pos: Vec3,
    /// Smoothed surface normal.
    pub up: Vec3,
    /// Body forward, tangent to the surface.
    pub facing: Vec3,
    /// Velocity along the surface.
    pub vel: Vec3,
    /// Body height above the contact point.
    pub hover: f32,
    /// How deep it has sunk into the sand (m).
    pub sink: f32,
    /// Smallest walkable normal.y (1 = flat only, -1 = anything).
    pub min_up: f32,
    pub on_glass: bool,
    /// Never leaves the glass (the starfish seen through the front pane).
    pub only_glass: bool,
    /// How much the body stays level instead of hugging the surface (0 = a
    /// crawler that follows every slope, ~0.8 = a swimmer gliding over it).
    pub upright: f32,
}

impl Crawler {
    pub fn new(pos: Vec3, facing: Vec3, hover: f32, min_up: f32, on_glass: bool) -> Self {
        Self {
            pos,
            up: Vec3::Y,
            facing: facing.normalize_or(Vec3::NEG_Z),
            vel: Vec3::ZERO,
            hover,
            sink: 0.0,
            min_up,
            on_glass,
            only_glass: false,
            upright: 0.0,
        }
    }

    pub fn swimmer(mut self, upright: f32) -> Self {
        self.upright = upright;
        self
    }

    fn walkable(&self, p: Vec3, n: Vec3) -> bool {
        let glass = p.x.abs() > HALF_W - 0.006 || p.z.abs() > HALF_D - 0.006;
        let surface = p.y > WATER_Y - 0.06;
        !surface && if glass { self.on_glass } else { !self.only_glass && n.y >= self.min_up }
    }

    /// Moves towards `desired` velocity (accel-limited), stays on the surface,
    /// turns `facing` towards `face` at `turn` rad/s. Returns false if blocked.
    pub fn step(&mut self, sdf: &DecorSdf, desired: Vec3, face: Vec3, accel: f32, turn: f32, dt: f32) -> bool {
        let tangent = |v: Vec3, n: Vec3| v - n * v.dot(n);
        let want = tangent(desired, self.up);
        let dv = (want - self.vel).clamp_length_max(accel * dt);
        self.vel = tangent(self.vel + dv, self.up);
        let mut blocked = false;
        // Walkers of the sand drop straight onto the ground; climbers (the
        // starfish) stick to whatever surface is nearest.
        let ground = |q: Vec3| -> (Vec3, Vec3) {
            if self.min_up > 0.5 {
                let p = sdf.drop_to_ground(q);
                (p, sdf.sample(p).1)
            } else {
                sdf.project(q + self.up * 0.004, 0.0)
            }
        };
        // Walkers never pop up or down: they climb what the slope allows (the
        // ground search could otherwise land them on a pebble's top or a ledge
        // above), and step off an edge by settling down at a finite speed.
        let settle = |from: Vec3, to: Vec3| -> Vec3 {
            if self.min_up > 0.5 && to.y < from.y {
                Vec3::new(to.x, to.y.max(from.y - 0.15 * dt), to.z)
            } else {
                to
            }
        };
        let rise_ok = |d: Vec3| self.min_up <= 0.5 || d.y <= Vec2::new(d.x, d.z).length() * 1.4 + 0.0015;
        if self.vel.length_squared() > 1e-10 {
            let (p, n) = ground(self.pos + self.vel * dt);
            // Somewhere it shouldn't be (pushed onto a rock slope): any move
            // that doesn't climb is fine.
            let (_, here) = sdf.sample(self.pos);
            let escaping = !self.walkable(self.pos, here);
            if (escaping || self.walkable(p, n)) && rise_ok(p - self.pos) && p.distance(self.pos) < 0.05 {
                self.pos = settle(self.pos, p);
            } else {
                blocked = true;
                self.vel *= 0.3;
            }
        } else {
            let p = ground(self.pos).0;
            if rise_ok(p - self.pos) {
                self.pos = settle(self.pos, p);
            }
        }
        let (_, n) = sdf.sample(self.pos);
        let n = n.lerp(Vec3::Y, self.upright).normalize_or(Vec3::Y);
        self.up = self.up.lerp(n, 1.0 - (-dt * 8.0).exp()).normalize_or(Vec3::Y);
        // Turn the body.
        let target = tangent(face, self.up).normalize_or(self.facing);
        let cur = tangent(self.facing, self.up).normalize_or(target);
        let angle = cur.angle_between(target);
        let k = if angle > 1e-4 { (turn * dt / angle).min(1.0) } else { 1.0 };
        self.facing = cur.slerp(target, k).normalize_or(cur);
        !blocked
    }

    pub fn transform(&self, scale: f32) -> Transform {
        let origin = self.pos + self.up * (self.hover - self.sink);
        Transform::from_translation(origin)
            .looking_to(self.facing, self.up)
            .with_scale(Vec3::splat(scale))
    }
}

/// A point near `p`, away from the rocks (horizontal SDF gradient 2 cm up).
pub fn away_from_rocks(sdf: &DecorSdf, p: Vec3, distance: f32) -> Vec3 {
    let (_, n) = sdf.sample(p + Vec3::Y * 0.02);
    let dir = Vec3::new(n.x, 0.0, n.z).normalize_or(Vec3::X);
    wander_target_with_room(sdf, p + dir * distance, 0.04, true, 0.03)
}

/// Whether a disc of `radius` lying on the sand at `p` is clear of rocks and
/// glass: probes 2.5 cm above the sand at the centre and around the rim (the
/// sand itself is then ~2.5 cm away; anything closer is decor).
pub fn disc_is_free(sdf: &DecorSdf, p: Vec3, radius: f32) -> bool {
    let probe = |q: Vec3| {
        let ground = Vec3::new(q.x, sand_height(q.x, q.z), q.z);
        sdf.distance(ground + Vec3::Y * 0.025) > 0.019
    };
    probe(p)
        && (0..8).all(|i| {
            let a = i as f32 / 8.0 * TAU;
            probe(p + Vec3::new(a.cos(), 0.0, a.sin()) * radius * 0.85)
        })
}

/// A random walkable point near `from`, on the surface (on open sand with at
/// least `room` of clearance when `sand_only`).

pub fn wander_target_with_room(sdf: &DecorSdf, from: Vec3, radius: f32, sand_only: bool, room: f32) -> Vec3 {
    // Ask for less room if the first tries find nothing.
    for room in [room, room * 0.6, 0.0] {
        let p = try_wander(sdf, from, radius, sand_only, room);
        if p != from {
            return p;
        }
    }
    from
}

fn try_wander(sdf: &DecorSdf, from: Vec3, radius: f32, sand_only: bool, room: f32) -> Vec3 {
    let mut r = rng();
    for _ in 0..40 {
        let a = r.random_range(0.0..TAU);
        let d = r.random_range(radius * 0.3..radius);
        let x = (from.x + a.cos() * d).clamp(-HALF_W + 0.06, HALF_W - 0.06);
        let z = (from.z + a.sin() * d).clamp(-HALF_D + 0.05, HALF_D - 0.05);
        let top = Vec3::new(x, WATER_Y - 0.08, z);
        let Some(t) = sdf.march(top, Vec3::NEG_Y, 0.6) else {
            continue;
        };
        let p = top - Vec3::Y * t;
        let (_, n) = sdf.sample(p);
        let sand = (p.y - sand_height(x, z)).abs() < 0.006;
        if n.y > 0.6 && (!sand_only || (sand && (room <= 0.0 || disc_is_free(sdf, p, room)))) {
            return p;
        }
    }
    from
}

// ---------------------------------------------------------------------------
// Hiding places: sand under rock overhangs, or nooks against a rock
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct HideSpots {
    pub spots: Vec<Vec3>,
    ready: bool,
}

fn find_hide_spots(sdf: Res<DecorSdf>, mut spots: ResMut<HideSpots>) {
    if !sdf.is_changed() && spots.ready {
        return;
    }
    let mut found: Vec<(Vec3, f32)> = Vec::new();
    let mut x = -HALF_W + 0.05;
    while x < HALF_W - 0.05 {
        let mut z = -HALF_D + 0.04;
        while z < HALF_D - 0.04 {
            let base = Vec3::new(x, sand_height(x, z), z);
            if sdf.distance(base + Vec3::Y * 0.008) > 0.004 {
                // Room for a body (1.5 cm tall), and something overhead facing
                // down (the underside of a rock) — not a wedge it can't enter.
                let room = sdf.distance(base + Vec3::Y * 0.012) > 0.011;
                let cover = room && [0.03f32, 0.04, 0.055].iter().any(|&h| {
                    let (d, n) = sdf.sample(base + Vec3::Y * h);
                    d < 0.009 && n.y < -0.15
                });
                // Rock right next to it (the sand alone would be 3 cm away).
                let nook = room && sdf.distance(base + Vec3::Y * 0.03) < 0.014;
                let score = cover as u32 as f32 * 2.0 + nook as u32 as f32;
                if score > 0.0 {
                    found.push((base, score));
                }
            }
            z += 0.015;
        }
        x += 0.015;
    }
    // Best cover first, spread over the whole tank (ties shuffled deterministically).
    let jitter = |p: Vec3| ((p.x * 127.1 + p.z * 311.7).sin() * 43758.5).fract();
    found.sort_by(|a, b| b.1.total_cmp(&a.1).then(jitter(a.0).total_cmp(&jitter(b.0))));
    spots.spots.clear();
    for (p, _) in found {
        if spots.spots.iter().all(|q| q.distance(p) > 0.09) {
            spots.spots.push(p);
        }
        if spots.spots.len() >= 16 {
            break;
        }
    }
    spots.ready = true;
    if sdf.complete && log_critters() {
        info!("critters: {} hiding places {:?}", spots.spots.len(), spots.spots);
    }
}

/// `AQ_LOG_CRITTERS=1`: log the bottom dwellers' decisions.
pub fn log_critters() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("AQ_LOG_CRITTERS").is_ok())
}

// ---------------------------------------------------------------------------
// Threats: the cursor finger, big fish, the wallpaper waking up
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct Threats {
    /// Positions of fish (and their length) this frame.
    pub fish: Vec<(Vec3, f32)>,
    /// The wallpaper just woke up: everybody startles.
    pub startle: bool,
}

impl Threats {
    /// Distance and escape direction of the closest threat within `reach`.
    pub fn near(&self, stimuli: &Stimuli, p: Vec3, reach: f32) -> Option<(f32, Vec3)> {
        let mut best: Option<(f32, Vec3)> = None;
        let mut consider = |d: f32, away: Vec3| {
            if d < reach && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, away.normalize_or(Vec3::X)));
            }
        };
        if let Some(c) = &stimuli.cursor {
            let ab = c.to - c.from;
            let s = ((p - c.from).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let q = c.from + ab * s;
            // A moving finger is scarier.
            let scare = 1.0 + (c.speed / 0.3).min(0.8);
            consider(p.distance(q) / scare, p - q);
        }
        for &(f, len) in &self.fish {
            // Only big fish (angelfish, discus) bother the bottom dwellers.
            if len > 0.075 {
                consider(p.distance(f) * 1.6, p - f);
            }
        }
        best
    }
}

/// `AQ_LOG_CRITTERS`: where everybody is, every 2 s.
fn log_positions(time: Res<Time>, mut next: Local<f32>, q: Query<(&Name, &Transform), With<MeshTag>>) {
    if !log_critters() || time.elapsed_secs() < *next {
        return;
    }
    *next = time.elapsed_secs() + 2.0;
    let mut line = format!("t={:.0}s", time.elapsed_secs());
    for (name, t) in &q {
        if matches!(name.as_str(), "crab" | "flatfish" | "starfish") {
            line += &format!(" {}({:.2},{:.2},{:.2})", &name.as_str()[..3], t.translation.x, t.translation.y, t.translation.z);
        }
    }
    info!("{line}");
}

fn sense_threats(
    time: Res<Time<Real>>,
    power: Res<Power>,
    mut forced_at: Local<f64>,
    fish: Query<(&Transform, &Fish)>,
    mut threats: ResMut<Threats>,
) {
    threats.fish.clear();
    threats.fish.extend(fish.iter().map(|(t, f)| (t.translation, f.length)));
    let now = time.elapsed_secs_f64();
    if FORCE_STARTLE.swap(false, std::sync::atomic::Ordering::Relaxed) {
        *forced_at = now;
    }
    // Minds tick at 4 Hz: the alarm lasts long enough for every one to notice.
    threats.startle = now - power.woke_at < 0.5 || now - *forced_at < 0.5;
}

/// Set by the physics test to startle everybody (as when the wallpaper wakes up).
pub static FORCE_STARTLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Sand puffs (burying, landing, scuttling): a small pool of grains
// ---------------------------------------------------------------------------

const GRAINS: usize = 72;

#[derive(Component, Default)]
struct Grain {
    active: bool,
    vel: Vec3,
    life: f32,
    size: f32,
}

#[derive(Resource, Default)]
pub struct SandPuffs {
    free: Vec<Entity>,
    /// Requests this frame: (position, strength 0..1).
    pub requests: Vec<(Vec3, f32)>,
}

fn spawn_puff_pool(
    mut commands: Commands,
    mut puffs: ResMut<SandPuffs>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap());
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.53, 0.40),
        perceptual_roughness: 0.95,
        ..default()
    });
    puffs.free.reserve(GRAINS);
    for i in 0..GRAINS {
        let warm = i == 0;
        let e = commands
            .spawn((
                Name::new("sand grain"),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(0.0, if warm { -0.03 } else { -1.0 }, 0.1).with_scale(Vec3::splat(0.001)),
                if warm { Visibility::Visible } else { Visibility::Hidden },
                Grain::default(),
                NotShadowCaster,
            ))
            .id();
        puffs.free.push(e);
    }
}

fn animate_puffs(
    time: Res<Time>,
    sdf: Res<DecorSdf>,
    mut puffs: ResMut<SandPuffs>,
    mut grains: Query<(Entity, &mut Grain, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs().min(0.05);
    let mut r = rng();
    let SandPuffs { free, requests } = &mut *puffs;
    for (at, strength) in requests.drain(..) {
        let n = (6.0 + 12.0 * strength) as usize;
        for _ in 0..n {
            let Some(e) = free.pop() else {
                break;
            };
            if let Ok((_, mut g, mut tr, mut vis)) = grains.get_mut(e) {
                let a = r.random_range(0.0..TAU);
                let out = Vec3::new(a.cos(), 0.0, a.sin());
                *g = Grain {
                    active: true,
                    vel: out * r.random_range(0.01..0.05) * (0.5 + strength) + Vec3::Y * r.random_range(0.02..0.07) * (0.5 + strength),
                    life: r.random_range(1.2..2.5),
                    size: r.random_range(0.0006..0.0013),
                };
                *tr = Transform::from_translation(at + out * 0.004 + Vec3::Y * 0.002).with_scale(Vec3::splat(g.size));
                *vis = Visibility::Visible;
            }
        }
    }
    for (e, mut g, mut tr, mut vis) in &mut grains {
        if !g.active {
            if *vis == Visibility::Visible && time.elapsed_secs() > 5.0 && tr.translation.y < 0.0 {
                *vis = Visibility::Hidden;
            }
            continue;
        }
        // Water drag and slow settling.
        g.vel += Vec3::NEG_Y * 0.09 * dt;
        g.vel *= (-dt * 3.0).exp();
        tr.translation += g.vel * dt;
        g.life -= dt;
        let landed = sdf.distance(tr.translation) < g.size;
        if g.life <= 0.0 || landed {
            g.active = false;
            *vis = Visibility::Hidden;
            free.push(e);
        }
    }
}

/// Spawns a critter entity with its material and tag.
pub fn critter_bundle(
    name: &'static str,
    mesh: Handle<Mesh>,
    material: Handle<CritterMaterial>,
    transform: Transform,
) -> impl Bundle {
    (Name::new(name), Mesh3d(mesh), MeshMaterial3d(material), MeshTag(0), transform)
}

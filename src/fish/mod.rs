//! Fish: four procedurally modelled species, swimming with a boids model
//! (separation / alignment / cohesion + Perlin wander + ray-cast avoidance of
//! the glass, rocks, sand and surface). The tail beat is done on the GPU; the
//! per-fish swim state is packed into the `MeshTag`.

mod boids;
mod bubbles;
mod mesh;
mod trace;

use std::f32::consts::TAU;

use bevy::{
    mesh::{MeshTag, MeshVertexBufferLayoutRef},
    pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};
use rand::{RngExt, SeedableRng, rngs::StdRng};

use crate::tank::{HALF_D, HALF_W};
use mesh::{Fin, Shape};

pub struct FishPlugin;

impl Plugin for FishPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<FishMaterial>::default())
            .init_resource::<bubbles::BubbleRequests>()
            .init_resource::<bubbles::BubblePool>()
            .add_systems(Startup, (load_fish_shaders, spawn_fish, bubbles::spawn_pool))
            .add_systems(
                Update,
                (boids::steer, boids::animate, bubbles::emit, bubbles::rise)
                    .chain()
                    .in_set(FishSystems),
            );
        trace::setup(app);
    }
}

/// The fish simulation (steering, tail beat, bubbles), for ordering.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FishSystems;

pub use bubbles::{Bubble, BubbleRequests};

#[derive(Resource)]
#[allow(dead_code)]
struct FishShaders(Vec<Handle<Shader>>);

fn load_fish_shaders(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(FishShaders(vec![assets.load("shaders/fish_swim.wgsl")]));
}

// ---------------------------------------------------------------------------
// Material
// ---------------------------------------------------------------------------

pub type FishMaterial = ExtendedMaterial<StandardMaterial, FishExt>;

#[derive(ShaderType, Clone, Copy, Debug, Default, Reflect)]
pub struct FishParams {
    /// x species id, y length, z tail amplitude, w body wavelength (in lengths).
    pub species: Vec4,
    pub color_a: Vec4,
    pub color_b: Vec4,
    pub color_c: Vec4,
    pub color_d: Vec4,
    /// x fin opacity, y iridescence, z scale sparkle, w body roughness.
    pub look: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct FishExt {
    #[uniform(100)]
    pub fish: FishParams,
}

impl MaterialExtension for FishExt {
    fn vertex_shader() -> ShaderRef {
        "shaders/fish.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/fish.wgsl".into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/fish_prepass.wgsl".into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        "shaders/fish_prepass.wgsl".into()
    }
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Fins are single sheets: render both sides in every pass.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

fn lin(r: f32, g: f32, b: f32) -> Vec4 {
    let c = Color::srgb(r, g, b).to_linear();
    Vec4::new(c.red, c.green, c.blue, 1.0)
}

// ---------------------------------------------------------------------------
// Species
// ---------------------------------------------------------------------------

/// Behaviour of a species.
#[derive(Clone, Copy)]
pub struct Behaviour {
    /// Cruising and maximum speed (m/s).
    pub cruise: f32,
    pub max_speed: f32,
    /// Maximum turn rate (rad/s) and acceleration (m/s²).
    pub turn_rate: f32,
    pub accel: f32,
    /// Preferred height band above the tank floor (m).
    pub depth: (f32, f32),
    /// Boids weights and radii.
    pub cohesion: f32,
    pub alignment: f32,
    pub separation: f32,
    pub view_radius: f32,
    pub wander: f32,
}

pub struct Species {
    pub name: &'static str,
    pub count: usize,
    pub shape: Shape,
    pub params: FishParams,
    pub behaviour: Behaviour,
}

/// Anemones planted in `scape::plants` (x, z) and their approximate top height.
pub const ANEMONES: [Vec3; 2] = [Vec3::new(0.02, 0.10, 0.105), Vec3::new(0.45, 0.08, 0.14)];

fn species_table() -> Vec<Species> {
    vec![
        Species {
            name: "neon tetra",
            count: 12,
            shape: Shape {
                length: 0.042,
                body: 0.78,
                height: 0.12,
                width: 0.06,
                hump: 0.42,
                head: 0.7,
                peduncle: 0.035,
                dorsal: Fin { from: 0.46, to: 0.6, height: 0.1, sweep: 0.4, shape: 0 },
                anal: Fin { from: 0.62, to: 0.9, height: 0.07, sweep: 0.3, shape: 0 },
                caudal_len: 0.25,
                caudal_h: 0.13,
                fork: 0.55,
                streamers: 0.0,
                pectoral: 0.08,
                pelvic: 0.0,
                eye: 0.055,
            },
            params: FishParams {
                species: Vec4::new(0.0, 0.042, 1.0, 1.0),
                color_a: lin(0.12, 0.85, 1.0),
                color_b: lin(1.0, 0.04, 0.08),
                color_c: lin(0.82, 0.84, 0.86),
                color_d: lin(0.55, 0.52, 0.4),
                look: Vec4::new(0.3, 55.0, 0.25, 0.35),
            },
            behaviour: Behaviour {
                cruise: 0.07,
                max_speed: 0.25,
                turn_rate: 3.2,
                accel: 0.6,
                depth: (0.10, 0.42),
                cohesion: 1.3,
                alignment: 1.1,
                separation: 1.6,
                view_radius: 0.16,
                wander: 0.55,
            },
        },
        Species {
            name: "angelfish",
            count: 3,
            shape: Shape {
                length: 0.085,
                body: 0.72,
                height: 0.3,
                width: 0.055,
                hump: 0.45,
                head: 0.6,
                peduncle: 0.05,
                dorsal: Fin { from: 0.28, to: 0.85, height: 0.72, sweep: 0.55, shape: 1 },
                anal: Fin { from: 0.38, to: 0.88, height: 0.68, sweep: 0.55, shape: 1 },
                caudal_len: 0.27,
                caudal_h: 0.2,
                fork: 0.25,
                streamers: 1.4,
                pectoral: 0.08,
                pelvic: 0.7,
                eye: 0.055,
            },
            params: FishParams {
                species: Vec4::new(1.0, 0.085, 0.55, 1.3),
                color_a: lin(0.78, 0.8, 0.82),
                color_b: lin(0.05, 0.05, 0.06),
                color_c: lin(0.95, 0.66, 0.2),
                color_d: lin(0.8, 0.84, 0.88),
                look: Vec4::new(0.72, 10.0, 0.2, 0.3),
            },
            behaviour: Behaviour {
                cruise: 0.035,
                max_speed: 0.12,
                turn_rate: 1.2,
                accel: 0.2,
                depth: (0.25, 0.52),
                cohesion: 0.35,
                alignment: 0.3,
                separation: 1.0,
                view_radius: 0.3,
                wander: 0.7,
            },
        },
        Species {
            name: "clownfish",
            count: 3,
            shape: Shape {
                length: 0.068,
                body: 0.8,
                height: 0.19,
                width: 0.09,
                hump: 0.36,
                head: 0.55,
                peduncle: 0.06,
                dorsal: Fin { from: 0.24, to: 0.84, height: 0.13, sweep: 0.2, shape: 2 },
                anal: Fin { from: 0.62, to: 0.86, height: 0.11, sweep: 0.2, shape: 0 },
                caudal_len: 0.2,
                caudal_h: 0.15,
                fork: -0.35,
                streamers: 0.0,
                pectoral: 0.11,
                pelvic: 0.0,
                eye: 0.06,
            },
            params: FishParams {
                species: Vec4::new(2.0, 0.068, 0.8, 1.0),
                color_a: lin(1.0, 0.36, 0.0),
                color_b: lin(0.97, 0.96, 0.93),
                color_c: lin(0.02, 0.02, 0.02),
                color_d: lin(1.0, 0.55, 0.2),
                look: Vec4::new(0.9, 0.0, 0.18, 0.3),
            },
            behaviour: Behaviour {
                cruise: 0.04,
                max_speed: 0.14,
                turn_rate: 2.4,
                accel: 0.4,
                depth: (0.06, 0.3),
                cohesion: 0.2,
                alignment: 0.2,
                separation: 1.2,
                view_radius: 0.12,
                wander: 0.9,
            },
        },
        Species {
            name: "discus",
            count: 2,
            shape: Shape {
                length: 0.11,
                body: 0.84,
                height: 0.4,
                width: 0.07,
                hump: 0.45,
                head: 0.55,
                peduncle: 0.05,
                dorsal: Fin { from: 0.2, to: 0.95, height: 0.11, sweep: 0.15, shape: 0 },
                anal: Fin { from: 0.35, to: 0.95, height: 0.1, sweep: 0.15, shape: 0 },
                caudal_len: 0.16,
                caudal_h: 0.14,
                fork: -0.3,
                streamers: 0.0,
                pectoral: 0.07,
                pelvic: 0.0,
                eye: 0.045,
            },
            params: FishParams {
                species: Vec4::new(3.0, 0.11, 0.45, 1.4),
                color_a: lin(0.9, 0.3, 0.06),
                color_b: lin(0.05, 0.8, 0.95),
                color_c: lin(0.5, 0.2, 0.08),
                color_d: lin(0.95, 0.3, 0.1),
                look: Vec4::new(0.6, 8.0, 0.15, 0.32),
            },
            behaviour: Behaviour {
                cruise: 0.03,
                max_speed: 0.1,
                turn_rate: 1.0,
                accel: 0.18,
                depth: (0.18, 0.45),
                cohesion: 0.25,
                alignment: 0.2,
                separation: 1.2,
                view_radius: 0.3,
                wander: 0.6,
            },
        },
    ]
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

#[derive(Component)]
pub struct Fish {
    pub species: usize,
    pub length: f32,
    pub velocity: Vec3,
    pub behaviour: Behaviour,
    pub seed: f64,
    /// Tail beat phase (rad) and current amplitude / angular frequency.
    pub phase: f32,
    pub amplitude: f32,
    pub omega: f32,
    /// Smoothed yaw rate, for body curl and banking.
    pub yaw_rate: f32,
    /// Speed multiplier from the wander noise (bursts / pauses).
    pub urge: f32,
    pub bubble_timer: f32,
    pub home: Option<Vec3>,
    /// Smoothed steering acceleration and angular velocity.
    pub steer: Vec3,
    pub ang_vel: Vec3,
    /// Side chosen to go around an obstacle (-1 / +1, 0 = none) and how long to keep it.
    pub avoid_side: f32,
    pub avoid_timer: f32,
    /// Fright (1 = full panic, decays to 0 in 2.2 s) and escape direction.
    pub panic: f32,
    pub flee: Vec3,
    /// 0 = hungry, >= 1 = full (decays over time).
    pub satiety: f32,
    /// Just snapped a flake (1 -> 0).
    pub gulp: f32,
    /// Debug: what steered the fish this frame (see `trace.rs`).
    pub events: u8,
}

fn spawn_fish(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FishMaterial>>,
) {
    let mut rng = StdRng::seed_from_u64(0xF15B);
    for (index, species) in species_table().into_iter().enumerate() {
        let mesh = meshes.add(mesh::fish_mesh(&species.shape));
        let material = materials.add(FishMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.35,
                reflectance: 0.5,
                double_sided: true,
                cull_mode: None,
                alpha_mode: AlphaMode::Mask(0.5),
                ..default()
            },
            extension: FishExt {
                fish: species.params,
            },
        });
        // Schooling species start together.
        let school_center = vec3(
            rng.random_range(-0.4..0.4),
            rng.random_range(species.behaviour.depth.0..species.behaviour.depth.1),
            rng.random_range(-0.1..0.15),
        );
        for k in 0..species.count {
            let home = if index == 2 {
                Some(ANEMONES[k % ANEMONES.len()])
            } else {
                None
            };
            let spread = if species.behaviour.cohesion > 1.0 { 0.08 } else { 0.35 };
            let mut p = school_center
                + vec3(
                    rng.random_range(-spread..spread),
                    rng.random_range(-0.04..0.04),
                    rng.random_range(-spread..spread) * 0.4,
                );
            if let Some(h) = home {
                p = h + vec3(rng.random_range(-0.05..0.05), 0.06, rng.random_range(-0.03..0.03));
            }
            p.x = p.x.clamp(-HALF_W + 0.1, HALF_W - 0.1);
            p.z = p.z.clamp(-HALF_D + 0.08, HALF_D - 0.08);
            p.y = p.y.clamp(0.08, 0.55);
            let heading = rng.random_range(0.0..TAU);
            let velocity =
                vec3(heading.cos(), 0.0, heading.sin() * 0.5) * species.behaviour.cruise;
            let scale = rng.random_range(0.85..1.12);
            commands.spawn((
                Name::new(species.name),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                MeshTag(0),
                Transform::from_translation(p)
                    .looking_to(velocity.normalize(), Vec3::Y)
                    .with_scale(Vec3::splat(scale)),
                Fish {
                    species: index,
                    length: species.shape.length * scale,
                    velocity,
                    behaviour: species.behaviour,
                    seed: rng.random_range(0.0..1000.0),
                    phase: rng.random_range(0.0..TAU),
                    amplitude: 0.5,
                    omega: 8.0,
                    yaw_rate: 0.0,
                    urge: 1.0,
                    bubble_timer: rng.random_range(4.0..30.0),
                    home,
                    steer: Vec3::ZERO,
                    ang_vel: Vec3::ZERO,
                    avoid_side: 0.0,
                    avoid_timer: 0.0,
                    panic: 0.0,
                    flee: Vec3::X,
                    satiety: 0.0,
                    gulp: 0.0,
                    events: 0,
                },
            ));
        }
    }
}

/// Packs the swim state for the shaders (see `fish_swim.wgsl`).
pub fn pack_swim_tag(phase: f32, amplitude: f32, omega: f32, turn: f32) -> u32 {
    let p = ((phase.rem_euclid(TAU) / TAU) * 4096.0) as u32 & 0xfff;
    let a = ((amplitude.clamp(0.0, 1.0) * 63.0).round() as u32) & 0x3f;
    let w = ((omega / 0.8).round().clamp(0.0, 63.0) as u32) & 0x3f;
    let t = (((turn.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u32) & 0xff;
    p | (a << 12) | (w << 18) | (t << 24)
}

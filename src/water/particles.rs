//! Suspended plankton / dust motes and rising micro-bubbles, animated on the GPU.

use bevy::{
    camera::visibility::NoFrustumCulling,
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};
use rand::{RngExt, SeedableRng, rngs::StdRng};

use crate::{
    config::AppConfig,
    meshgen::MeshBuilder,
    scape::sand_height,
    tank::{HALF_D, HALF_W, WATER_Y},
};

pub struct ParticlesPlugin;

impl Plugin for ParticlesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<ParticleMaterial>::default())
            .add_systems(Startup, spawn_particles)
            .add_systems(Update, apply_particle_budget);
    }
}

#[derive(ShaderType, Clone, Copy, Debug, Default)]
pub struct ParticleParams {
    /// rgb tint, a brightness.
    pub color: Vec4,
    /// x kind (0 motes, 1 bubbles), y size, z density, w water surface y.
    pub params: Vec4,
    /// Tank half extents, w speed.
    pub bounds: Vec4,
    /// The cursor "finger" in the water: segment start (xyz) + strength (w),
    /// end (xyz) + radius (w), and its motion direction.
    pub cursor_a: Vec4,
    pub cursor_b: Vec4,
    pub cursor_v: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct ParticleMaterial {
    #[uniform(0)]
    pub particles: ParticleParams,
}

impl Material for ParticleMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/particles.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/particles.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

/// Emitters of bubble streams (x, z), near the stones.
const BUBBLE_VENTS: [(f32, f32); 5] = [
    (-0.30, -0.02),
    (0.36, -0.10),
    (-0.54, 0.05),
    (0.05, -0.19),
    (0.52, -0.16),
];

/// Quads sharing a centre, one per particle; `uv_b` carries two random seeds.
fn particle_quads(centers: &[(Vec3, Vec2)]) -> Mesh {
    let mut b = MeshBuilder::default();
    for (c, seed) in centers {
        for uv in [vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0)] {
            b.vertex(*c, Vec3::Z, uv);
            b.uvs_b.push(seed.to_array());
        }
        let i = b.len() - 4;
        b.quad(i, i + 1, i + 2, i + 3);
    }
    b.build()
}

#[derive(Component)]
struct Particles;

fn spawn_particles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ParticleMaterial>>,
) {
    let mut rng = StdRng::seed_from_u64(7);
    let half = vec3(HALF_W - 0.01, 0.0, HALF_D - 0.01);
    let bounds = |speed: f32| half.extend(speed);

    // Motes: fine dust that glitters in the light shafts + larger pale plankton.
    let mut motes = Vec::new();
    for _ in 0..3500 {
        let p = vec3(
            rng.random_range(-half.x..half.x),
            rng.random_range(0.02..WATER_Y - 0.01),
            rng.random_range(-half.z..half.z),
        );
        motes.push((p, vec2(rng.random(), rng.random())));
    }
    let mut plankton = Vec::new();
    for _ in 0..500 {
        let p = vec3(
            rng.random_range(-half.x..half.x),
            rng.random_range(0.05..WATER_Y - 0.02),
            rng.random_range(-half.z..half.z),
        );
        plankton.push((p, vec2(rng.random(), rng.random())));
    }
    let mut bubbles = Vec::new();
    for (x, z) in BUBBLE_VENTS {
        let y = sand_height(x, z) + 0.01;
        for _ in 0..36 {
            bubbles.push((vec3(x, y, z), vec2(rng.random(), rng.random())));
        }
    }

    let sets = [
        (
            "dust motes",
            motes,
            ParticleParams {
                color: vec4(1.0, 0.97, 0.9, 900.0),
                params: vec4(0.0, 0.0011, 1.0, WATER_Y),
                bounds: bounds(1.0),
                ..default()
            },
        ),
        (
            "plankton",
            plankton,
            ParticleParams {
                color: vec4(0.75, 1.0, 0.85, 500.0),
                params: vec4(0.0, 0.0024, 1.0, WATER_Y),
                bounds: bounds(0.6),
                ..default()
            },
        ),
        (
            "micro bubbles",
            bubbles,
            ParticleParams {
                color: vec4(0.85, 0.95, 1.0, 1400.0),
                params: vec4(1.0, 0.0016, 1.0, WATER_Y),
                bounds: bounds(1.0),
                ..default()
            },
        ),
    ];
    for (name, centers, params) in sets {
        commands.spawn((
            Name::new(name),
            Mesh3d(meshes.add(particle_quads(&centers))),
            MeshMaterial3d(materials.add(ParticleMaterial { particles: params })),
            Particles,
            NoFrustumCulling,
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
}

/// Low-power mode halves the particle counts.
fn apply_particle_budget(
    config: Res<AppConfig>,
    query: Query<&MeshMaterial3d<ParticleMaterial>, With<Particles>>,
    mut materials: ResMut<Assets<ParticleMaterial>>,
) {
    let density = if config.low_power { 0.5 } else { 1.0 };
    for handle in &query {
        let needs_update = materials
            .get(&handle.0)
            .is_some_and(|m| (m.particles.params.z - density).abs() > 1e-3);
        if needs_update {
            if let Some(mut m) = materials.get_mut(&handle.0) {
                m.particles.params.z = density;
            }
        }
    }
}

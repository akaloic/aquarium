//! The aquascape: sand bed, scanned rocks & driftwood with procedural moss, and
//! procedural swaying plants / anemones.

mod plants;
mod rocks;
mod sand;
pub mod scans;

use std::sync::OnceLock;

use bevy::{
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use crate::tank::{HALF_D, HALF_W};

pub struct ScapePlugin;

impl Plugin for ScapePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<RockMaterial>::default(),
            MaterialPlugin::<PlantMaterial>::default(),
            scans::ScansPlugin,
        ))
        .add_systems(
            Startup,
            (sand::spawn_sand, rocks::spawn_hardscape, plants::spawn_plants),
        )
        .add_systems(Update, plants::fit_plants);
    }
}

// ---------------------------------------------------------------------------
// Materials
// ---------------------------------------------------------------------------

pub type RockMaterial = ExtendedMaterial<StandardMaterial, MossExt>;

#[derive(ShaderType, Clone, Copy, Debug, Default, Reflect)]
pub struct MossParams {
    /// Linear RGB moss colour.
    pub color: Vec4,
    /// x: coverage, y: noise scale, z: seed, w: wetness.
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct MossExt {
    #[uniform(100)]
    pub moss: MossParams,
}

impl MaterialExtension for MossExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/moss.wgsl".into()
    }
}

pub type PlantMaterial = ExtendedMaterial<StandardMaterial, SwayExt>;

#[derive(ShaderType, Clone, Copy, Debug, Default, Reflect)]
pub struct SwayParams {
    pub amplitude: f32,
    pub frequency: f32,
    pub water_y: f32,
    /// 0 = current-driven plants, 1 = tentacles.
    pub mode: f32,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct SwayExt {
    #[uniform(100)]
    pub sway: SwayParams,
}

impl MaterialExtension for SwayExt {
    fn vertex_shader() -> ShaderRef {
        "shaders/plant.wgsl".into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/plant_prepass.wgsl".into()
    }
}

// ---------------------------------------------------------------------------
// Substrate height field, shared by the sand mesh and object placement
// ---------------------------------------------------------------------------

/// Hero stones, used to raise the sand around them: (x, z, radius, height).
const MOUNDS: [(f32, f32, f32, f32); 4] = [
    (-0.24, -0.04, 0.17, 0.028),
    (0.33, -0.08, 0.15, 0.022),
    (-0.50, 0.07, 0.11, 0.014),
    (0.05, -0.20, 0.20, 0.012),
];

fn fbm() -> &'static Fbm<Perlin> {
    static FBM: OnceLock<Fbm<Perlin>> = OnceLock::new();
    FBM.get_or_init(|| Fbm::<Perlin>::new(11).set_octaves(4).set_frequency(6.0))
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Height of the sand surface above the tank floor at (x, z).
pub fn sand_height(x: f32, z: f32) -> f32 {
    // Classic aquascape slope: shallow at the front, deep at the back.
    let mut h = 0.030 + 0.068 * smoothstep(HALF_D - 0.02, -HALF_D + 0.04, z);
    // Slightly higher on the sides, framing the scene.
    h += 0.012 * smoothstep(0.35, HALF_W, x.abs());
    for (mx, mz, r, mh) in MOUNDS {
        let d2 = (x - mx).powi(2) + (z - mz).powi(2);
        h += mh * (-d2 / (r * r)).exp();
    }
    // A shallow winding "path" from the front centre towards the back right.
    let path_x = 0.06 + 0.10 * (1.0 - (z + HALF_D) / (2.0 * HALF_D)).powf(1.5) + 0.03 * (z * 9.0).sin();
    let dp = (x - path_x).abs();
    h -= 0.013 * (1.0 - smoothstep(0.03, 0.11, dp)) * smoothstep(-0.25, 0.1, z);
    // Natural unevenness.
    let n = fbm().get([x as f64, z as f64]) as f32;
    h += n * 0.006;
    h.max(0.012)
}

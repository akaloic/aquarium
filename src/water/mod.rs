//! The water: volumetric medium with light shafts, animated surface (both
//! sides), projected caustics and suspended particles.

mod caustics;
mod materials;
mod particles;

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    light::{FogVolume, NotShadowCaster, NotShadowReceiver},
    pbr::ExtendedMaterial,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

pub use caustics::Cookie;
pub use materials::*;
pub use particles::ParticleMaterial;

use crate::{
    meshgen::MeshBuilder,
    tank::{CORNER_R, HALF_D, HALF_W, WATER_Y},
};

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            particles::ParticlesPlugin,
            caustics::CausticsPlugin,
            MaterialPlugin::<WaterSurfaceMaterial>::default(),
            MaterialPlugin::<WaterUndersideMaterial>::default(),
        ))
        .add_systems(Startup, (load_shader_libraries, spawn_water))
        .add_systems(Update, drift_fog_density);

        // Swap Bevy's volumetric fog shader for our single-light water version
        // (same bindings, much cheaper per ray-march step).
        app.world()
            .resource::<bevy::asset::io::embedded::EmbeddedAssetRegistry>()
            .insert_asset(
                std::path::PathBuf::new(),
                std::path::Path::new("bevy_pbr/volumetric_fog/volumetric_fog.wgsl"),
                include_bytes!("../../assets/shaders/volumetric_water.wgsl").as_slice(),
            );
    }
}

/// Keeps `#import`-able WGSL modules alive.
#[derive(Resource)]
#[allow(dead_code)]
struct ShaderLibraries(Vec<Handle<Shader>>);

fn load_shader_libraries(mut commands: Commands, assets: Res<AssetServer>) {
    let libs = ["noise", "caustics", "ripples", "sway"]
        .iter()
        .map(|name| assets.load(format!("shaders/{name}.wgsl")))
        .collect();
    commands.insert_resource(ShaderLibraries(libs));
}

#[derive(Component)]
pub struct WaterFog;

fn spawn_water(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut surface_materials: ResMut<Assets<WaterSurfaceMaterial>>,
    mut underside_materials: ResMut<Assets<WaterUndersideMaterial>>,
) {
    // --- Participating medium: fills the tank up to the water line ---
    let inset = 0.004;
    let size = vec3(2.0 * (HALF_W - inset), WATER_Y, 2.0 * (HALF_D - inset));
    commands.spawn((
        Name::new("water volume"),
        FogVolume {
            fog_color: Color::srgb(0.36, 0.80, 0.95),
            light_tint: Color::srgb(0.9, 1.0, 1.0),
            density_factor: 0.72,
            density_texture: Some(images.add(water_density_texture())),
            absorption: 0.38,
            scattering: 0.5,
            // Our fog shader has no directional light: this field carries the
            // water surface height used to shape the light shafts.
            scattering_asymmetry: WATER_Y,
            light_intensity: 0.75,
            ..default()
        },
        Transform::from_xyz(0.0, WATER_Y * 0.5, 0.0).with_scale(size),
        WaterFog,
    ));

    // --- Surface seen from above (transmissive, IOR 1.33) ---
    commands.spawn((
        Name::new("water surface"),
        Mesh3d(meshes.add(rounded_rect(HALF_W, HALF_D, CORNER_R, WATER_Y, true))),
        MeshMaterial3d(surface_materials.add(ExtendedMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                specular_transmission: 1.0,
                ior: 1.33,
                thickness: 0.05,
                perceptual_roughness: 0.0,
                reflectance: 0.35,
                attenuation_color: Color::srgb(0.55, 0.85, 0.85),
                attenuation_distance: 1.5,
                ..default()
            },
            extension: WaterSurfaceExt {
                ripple: vec4(1.0, 1.0, 1.5, 0.0),
                drops: [Vec4::ZERO; 4],
            },
        })),
        NotShadowCaster,
        NotShadowReceiver,
    ));

    // --- Surface seen from below (total internal reflection / Snell's window) ---
    commands.spawn((
        Name::new("water underside"),
        Mesh3d(meshes.add(rounded_rect(HALF_W, HALF_D, CORNER_R, WATER_Y - 0.0005, false))),
        MeshMaterial3d(underside_materials.add(WaterUndersideMaterial {
            params: UndersideParams {
                mirror_color: vec4(1.2, 4.2, 5.0, 0.0),
                floor_color: vec4(9.0, 11.0, 10.0, 0.0),
                sky_color: vec4(1.5, 1.8, 2.2, 0.0),
                light: vec4(0.08, 2.05, 0.20, 9000.0),
                ripple: vec4(1.4, 1.0, 0.0, 0.0),
                drops: [Vec4::ZERO; 4],
            },
        })),
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

/// Flat rounded rectangle (triangle fan) at height `y`, facing up or down.
pub fn rounded_rect(half_w: f32, half_d: f32, r: f32, y: f32, up: bool) -> Mesh {
    let mut b = MeshBuilder::default();
    let n = if up { Vec3::Y } else { -Vec3::Y };
    let center = b.vertex(vec3(0.0, y, 0.0), n, Vec2::ZERO);
    let segs = 10;
    let corners = [
        (vec2(half_w - r, half_d - r), 0.0_f32),
        (vec2(-half_w + r, half_d - r), 0.5 * std::f32::consts::PI),
        (vec2(-half_w + r, -half_d + r), std::f32::consts::PI),
        (vec2(half_w - r, -half_d + r), 1.5 * std::f32::consts::PI),
    ];
    let mut ring = Vec::new();
    for (c, a0) in corners {
        for i in 0..=segs {
            let a = a0 + 0.5 * std::f32::consts::PI * i as f32 / segs as f32;
            let p = c + vec2(a.cos(), a.sin()) * r;
            ring.push(b.vertex(vec3(p.x, y, p.y), n, p));
        }
    }
    for i in 0..ring.len() {
        b.tri(center, ring[i], ring[(i + 1) % ring.len()]);
    }
    b.orient_to_normals();
    b.build()
}

/// Density of the water medium: denser (more suspended matter) with depth,
/// plus soft turbid clouds. Tileable in X and Z so it can drift.
fn water_density_texture() -> Image {
    use noise::{NoiseFn, Perlin};
    let (nx, ny, nz) = (48u32, 24u32, 24u32);
    let perlin = Perlin::new(7);
    let mut data = Vec::with_capacity((nx * ny * nz) as usize);
    let tau = std::f64::consts::TAU;
    for z in 0..nz {
        for y in 0..ny {
            for x in 0..nx {
                // Map x and z on circles so the noise tiles.
                let (ax, az) = (x as f64 / nx as f64 * tau, z as f64 / nz as f64 * tau);
                let p = [
                    ax.cos() * 1.2,
                    ax.sin() * 1.2 + az.cos() * 0.8,
                    az.sin() * 0.8 + y as f64 * 0.18,
                ];
                let n = perlin.get(p) * 0.5 + perlin.get([p[0] * 2.3, p[1] * 2.3, p[2] * 2.3]) * 0.25;
                // y = 0 is the bottom of the volume.
                let depth = 1.0 - y as f32 / (ny - 1) as f32;
                let strat = 0.55 + 0.45 * depth.powf(1.3);
                let d = strat * (0.85 + 0.6 * n as f32);
                data.push((d.clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: nx,
            height: ny,
            depth_or_array_layers: nz,
        },
        TextureDimension::D3,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// Slowly drifts the turbidity so the medium feels alive.
fn drift_fog_density(time: Res<Time>, mut fog: Query<&mut FogVolume, With<WaterFog>>) {
    let t = time.elapsed_secs();
    for mut f in &mut fog {
        f.density_texture_offset = vec3(t * 0.004, 0.0, (t * 0.13).sin() * 0.03);
    }
}

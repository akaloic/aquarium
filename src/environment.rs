//! Lighting rig and the procedural "dark gallery" environment used for reflections.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageSampler, ImageSamplerDescriptor},
    light::{GeneratedEnvironmentMapLight, Skybox, VolumetricLight},
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
    },
};

use crate::tank::WATER_Y;

/// Scale applied to the procedural environment radiance (cd/m² per unit).
pub const ENV_INTENSITY: f32 = 450.0;

pub struct EnvironmentPlugin;

impl Plugin for EnvironmentPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.55, 0.75, 1.0),
            brightness: 6.0,
            ..default()
        })
        .add_systems(Startup, spawn_lights);
        // Built immediately so that the camera can reference it at startup.
        let cubemap = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(dark_room_cubemap(256));
        app.insert_resource(EnvironmentCubemap(cubemap));
    }
}

/// Handle to the environment cubemap, attached to the camera by `camera.rs`.
#[derive(Resource, Clone)]
pub struct EnvironmentCubemap(pub Handle<Image>);

#[derive(Component)]
pub struct MainSpot;

fn spawn_lights(mut commands: Commands) {
    // Main aquarium luminaire: cool white spot high above the water. It projects
    // the animated caustics texture (water/caustics.rs) and lights the fog.
    commands.spawn((
        Name::new("main spot"),
        SpotLight {
            color: Color::srgb(0.86, 0.94, 1.0),
            intensity: 80_000.0,
            range: 6.0,
            radius: 0.012,
            shadow_maps_enabled: true,
            soft_shadows_enabled: true,
            shadow_depth_bias: 0.01,
            shadow_normal_bias: 1.2,
            // Must stay at the default 0.1: Bevy's volumetric fog shader assumes it.
            shadow_map_near_z: 0.1,
            inner_angle: 0.36,
            outer_angle: 0.62,
            ..default()
        },
        Transform::from_xyz(0.08, 2.05, 0.20).looking_at(vec3(0.0, 0.0, -0.04), Vec3::Y),
        VolumetricLight,
        MainSpot,
    ));

    // One very soft teal fill light hidden under the "hood", no shadows: keeps the
    // shadowed parts of the scape from going pitch black. Short range, so it
    // only costs something inside the tank.
    commands.spawn((
        Name::new("fill"),
        PointLight {
            color: Color::srgb(0.35, 0.75, 0.95),
            intensity: 700.0,
            range: 1.1,
            radius: 0.3,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, WATER_Y + 0.04, 0.12),
    ));
}

/// Components to attach to the main camera for reflections / background.
pub fn camera_environment(env: &EnvironmentCubemap) -> impl Bundle {
    (
        GeneratedEnvironmentMapLight {
            environment_map: env.0.clone(),
            intensity: ENV_INTENSITY,
            ..default()
        },
        // The visible background is kept darker than what the glass reflects.
        Skybox {
            image: Some(env.0.clone()),
            brightness: ENV_INTENSITY * 0.3,
            ..default()
        },
    )
}

/// A rectangular emitter in direction space.
struct Softbox {
    dir: Vec3,
    half_w: f32,
    half_h: f32,
    radiance: Vec3,
}

impl Softbox {
    fn eval(&self, d: Vec3) -> Vec3 {
        let f = self.dir.normalize();
        let right = f.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(f);
        let c = d.dot(f);
        if c <= 0.0 {
            return Vec3::ZERO;
        }
        let x = d.dot(right) / c;
        let y = d.dot(up) / c;
        // Soft edges (diffuser).
        let ex = 1.0 - smoothstep(self.half_w * 0.85, self.half_w, x.abs());
        let ey = 1.0 - smoothstep(self.half_h * 0.85, self.half_h, y.abs());
        self.radiance * ex * ey
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Radiance of a dim gallery room, relative units (scaled by `ENV_INTENSITY`).
fn room_radiance(d: Vec3) -> Vec3 {
    let boxes = [
        // Large diffuser up-left in front of the tank.
        Softbox {
            dir: vec3(-0.55, 0.55, 0.62),
            half_w: 0.42,
            half_h: 0.22,
            radiance: vec3(1.6, 1.62, 1.7),
        },
        // Tall strip light on the right.
        Softbox {
            dir: vec3(0.92, 0.15, 0.30),
            half_w: 0.06,
            half_h: 0.55,
            radiance: vec3(1.2, 1.25, 1.35),
        },
        // Warm dim doorway behind-left.
        Softbox {
            dir: vec3(-0.7, 0.05, -0.7),
            half_w: 0.18,
            half_h: 0.4,
            radiance: vec3(0.10, 0.07, 0.045),
        },
    ];
    // Base: dark walls, slightly lighter ceiling, very dark floor.
    let up = d.y;
    let wall = vec3(0.010, 0.011, 0.013);
    let ceiling = vec3(0.016, 0.018, 0.022);
    let floor = vec3(0.004, 0.004, 0.004);
    let mut c = if up >= 0.0 {
        wall.lerp(ceiling, smoothstep(0.0, 0.8, up))
    } else {
        wall.lerp(floor, smoothstep(0.0, 0.25, -up))
    };
    for b in &boxes {
        c += b.eval(d);
    }
    c
}

/// Builds an RGBA16F cubemap of `room_radiance`.
fn dark_room_cubemap(size: u32) -> Image {
    let mut data: Vec<u8> = Vec::with_capacity((size * size * 6 * 8) as usize);
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let u = 2.0 * (x as f32 + 0.5) / size as f32 - 1.0;
                let v = 2.0 * (y as f32 + 0.5) / size as f32 - 1.0;
                let d = match face {
                    0 => vec3(1.0, -v, -u),
                    1 => vec3(-1.0, -v, u),
                    2 => vec3(u, 1.0, v),
                    3 => vec3(u, -1.0, -v),
                    4 => vec3(u, -v, 1.0),
                    _ => vec3(-u, -v, -1.0),
                }
                .normalize();
                let c = room_radiance(d);
                for ch in [c.x, c.y, c.z, 1.0] {
                    data.extend_from_slice(&half::f16::from_f32(ch).to_le_bytes());
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}

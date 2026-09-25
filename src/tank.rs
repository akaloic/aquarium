//! The glass tank (bent-glass corners), its cabinet, the background film and the dark room.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageSampler, ImageSamplerDescriptor},
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::MeshVertexBufferLayoutRef,
    pbr::{
        ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
        MaterialPipeline, MaterialPipelineKey,
    },
    prelude::*,
    shader::ShaderRef,
    render::render_resource::{
        AsBindGroup, Extent3d, RenderPipelineDescriptor, SpecializedMeshPipelineError,
        TextureDimension, TextureFormat,
    },
};

use crate::meshgen::MeshBuilder;

/// Inner half width (x) of the water volume, metres.
pub const HALF_W: f32 = 0.70;
/// Inner half depth (z).
pub const HALF_D: f32 = 0.30;
/// Height of the glass walls above the tank floor (y = 0 is the inner floor).
pub const GLASS_H: f32 = 0.72;
/// Water surface height.
pub const WATER_Y: f32 = 0.645;
/// Glass thickness.
pub const GLASS_T: f32 = 0.012;
/// Inner radius of the bent vertical corners.
pub const CORNER_R: f32 = 0.05;
/// Height of the cabinet top (the tank bottom sits on it).
pub const CABINET_TOP: f32 = -GLASS_T;
pub const FLOOR_Y: f32 = -0.86;

pub struct TankPlugin;

impl Plugin for TankPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<GlassMaterial>::default(),
            MaterialPlugin::<PaneMaterial>::default(),
        ))
            .add_systems(Startup, spawn_tank);
    }
}

/// Flat panes: Fresnel reflection of the room blended over the scene (see
/// `glass_pane.wgsl`). The bent corners keep real screen-space transmission.
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct PaneMaterial {
    #[uniform(0)]
    pub params: Vec4,
    #[uniform(0)]
    pub tint: Vec4,
    #[texture(1, dimension = "cube")]
    #[sampler(2)]
    pub environment: Handle<Image>,
}

impl Material for PaneMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/glass_pane.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        disable_depth_write(descriptor);
        Ok(())
    }
}

/// Transmissive material that does not write depth, so that the volumetric water
/// behind it is computed against the real underwater geometry.
pub type GlassMaterial = ExtendedMaterial<StandardMaterial, NoDepthWrite>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct NoDepthWrite {}

impl MaterialExtension for NoDepthWrite {
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        disable_depth_write(descriptor);
        Ok(())
    }
}

pub fn disable_depth_write(descriptor: &mut RenderPipelineDescriptor) {
    // Only the colour pass: shadow / prepass pipelines have no colour targets.
    let has_color = descriptor
        .fragment
        .as_ref()
        .is_some_and(|f| f.targets.iter().any(Option::is_some));
    if has_color {
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(false);
        }
    }
}

/// A wall segment of the tank, described by its inner outline in the XZ plane.
struct Wall {
    /// Points along the inner glass surface with their outward normals.
    outline: Vec<(Vec2, Vec2)>,
    corner: bool,
}

fn walls() -> Vec<Wall> {
    let (ix, iz) = (HALF_W - CORNER_R, HALF_D - CORNER_R);
    let mut out = Vec::new();
    // Flat panes: front (+z), back (-z), right (+x), left (-x).
    let flat = |a: Vec2, b: Vec2, n: Vec2| Wall {
        outline: vec![(a, n), (b, n)],
        corner: false,
    };
    out.push(flat(vec2(-ix, HALF_D), vec2(ix, HALF_D), Vec2::Y));
    out.push(flat(vec2(ix, -HALF_D), vec2(-ix, -HALF_D), -Vec2::Y));
    out.push(flat(vec2(HALF_W, iz), vec2(HALF_W, -iz), Vec2::X));
    out.push(flat(vec2(-HALF_W, -iz), vec2(-HALF_W, iz), -Vec2::X));
    // Bent corners (quarter cylinders), counter-clockwise when seen from above
    // is not required: each piece is independent.
    let corners = [
        (vec2(ix, iz), 0.0_f32),            // front-right: from +z to +x
        (vec2(-ix, iz), 0.5 * std::f32::consts::PI), // front-left
        (vec2(-ix, -iz), std::f32::consts::PI), // back-left
        (vec2(ix, -iz), 1.5 * std::f32::consts::PI), // back-right
    ];
    for (c, a0) in corners {
        let segs = 14;
        let mut outline = Vec::new();
        for i in 0..=segs {
            // Angle measured from +x axis in the XZ plane (x = cos, z = sin).
            let a = a0 + 0.5 * std::f32::consts::PI * i as f32 / segs as f32;
            let n = vec2(a.cos(), a.sin());
            outline.push((c + n * CORNER_R, n));
        }
        out.push(Wall { outline, corner: true });
    }
    out
}

/// Builds the inner + outer surfaces of a wall (and separately its top edge).
fn wall_meshes(wall: &Wall) -> (Mesh, Mesh, Vec3) {
    let mut glass = MeshBuilder::default();
    let mut edge = MeshBuilder::default();
    let (y0, y1) = (-GLASS_T, GLASS_H);
    let n = wall.outline.len();
    let mut s = 0.0;
    let mut center = Vec3::ZERO;
    for i in 0..n {
        let (p, nrm) = wall.outline[i];
        if i > 0 {
            s += (p - wall.outline[i - 1].0).length();
        }
        let inner = vec3(p.x, 0.0, p.y);
        let outer = inner + vec3(nrm.x, 0.0, nrm.y) * GLASS_T;
        let n3 = vec3(nrm.x, 0.0, nrm.y);
        center += inner;
        // outer surface
        glass.vertex(outer.with_y(y0), n3, vec2(s, 1.0));
        glass.vertex(outer.with_y(y1), n3, vec2(s, 0.0));
        // inner surface
        glass.vertex(inner.with_y(y0), -n3, vec2(s, 1.0));
        glass.vertex(inner.with_y(y1), -n3, vec2(s, 0.0));
        // top edge
        edge.vertex(inner.with_y(y1), Vec3::Y, vec2(s, 0.0));
        edge.vertex(outer.with_y(y1), Vec3::Y, vec2(s, 1.0));
    }
    center /= n as f32;
    for i in 0..(n as u32 - 1) {
        let (a, b) = (i * 4, (i + 1) * 4);
        // Outer: normal points outward. Winding chosen so the face is CCW from outside.
        glass.quad(a, b, b + 1, a + 1);
        // Inner: CCW from inside.
        glass.quad(a + 2, a + 3, b + 3, b + 2);
        let (ea, eb) = (i * 2, (i + 1) * 2);
        edge.quad(ea, ea + 1, eb + 1, eb);
    }
    // Ensure the winding really faces the normals (outline direction differs per wall).
    let fix = |mut m: MeshBuilder| {
        m.orient_to_normals();
        m.build()
    };
    (fix(glass), fix(edge), center)
}

fn spawn_tank(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut glass_materials: ResMut<Assets<GlassMaterial>>,
    mut pane_materials: ResMut<Assets<PaneMaterial>>,
    env: Res<crate::environment::EnvironmentCubemap>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let glass_base = StandardMaterial {
        base_color: Color::srgb(0.97, 1.0, 0.99),
        specular_transmission: 0.96,
        diffuse_transmission: 0.0,
        // Air -> water interface: the thin glass itself barely deviates rays.
        ior: 1.33,
        thickness: GLASS_T,
        // 0 selects the single-tap (non-blurred) screen-space transmission path.
        perceptual_roughness: 0.0,
        reflectance: 0.5,
        attenuation_color: Color::srgb(0.80, 0.96, 0.92),
        attenuation_distance: 0.35,
        ..default()
    };
    let pane = pane_materials.add(PaneMaterial {
        params: Vec4::new(0.04, crate::environment::ENV_INTENSITY, 0.012, 0.0),
        tint: Vec4::new(0.35, 0.75, 0.6, 0.0),
        environment: env.0.clone(),
    });
    // Water-filled bent corners act as cylindrical lenses: fake it with a thicker volume.
    let corner = glass_materials.add(GlassMaterial {
        base: StandardMaterial {
            thickness: 0.07,
            ..glass_base.clone()
        },
        extension: NoDepthWrite {},
    });
    // Polished top edges look green (long path through the glass).
    let edge = glass_materials.add(GlassMaterial {
        base: StandardMaterial {
            base_color: Color::srgb(0.55, 0.85, 0.75),
            thickness: 0.3,
            attenuation_distance: 0.12,
            attenuation_color: Color::srgb(0.45, 0.85, 0.70),
            perceptual_roughness: 0.12,
            ..glass_base
        },
        extension: NoDepthWrite {},
    });

    for wall in walls() {
        let (glass_mesh, edge_mesh, center) = wall_meshes(&wall);
        // Entities are positioned at their centre so that transmissive sorting
        // (back to front, by entity distance) is correct.
        let offset = Transform::from_translation(center);
        let recenter = |mut m: Mesh| {
            m.translate_by(-center);
            m
        };
        let mut glass = commands.spawn((
            Name::new("glass"),
            Mesh3d(meshes.add(recenter(glass_mesh))),
            offset,
            NotShadowCaster,
            NotShadowReceiver,
        ));
        if wall.corner {
            glass.insert(MeshMaterial3d(corner.clone()));
        } else {
            glass.insert(MeshMaterial3d(pane.clone()));
        }
        commands.spawn((
            Name::new("glass edge"),
            Mesh3d(meshes.add(recenter(edge_mesh))),
            MeshMaterial3d(edge.clone()),
            offset,
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }

    // --- Background film glued behind the back pane (classic dark aquarium backdrop) ---
    let film_w = 2.0 * (HALF_W - CORNER_R) + 0.01;
    let film_h = GLASS_H + GLASS_T;
    commands.spawn((
        Name::new("background film"),
        Mesh3d(meshes.add(Rectangle::new(film_w, film_h))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(images.add(backdrop_gradient())),
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(0.0, film_h * 0.5 - GLASS_T, -HALF_D - GLASS_T - 0.002),
        NotShadowCaster,
    ));

    // --- Cabinet (black lacquer) ---
    let cab_w = 2.0 * (HALF_W + GLASS_T) + 0.06;
    let cab_d = 2.0 * (HALF_D + GLASS_T) + 0.08;
    let cab_h = CABINET_TOP - FLOOR_Y;
    commands.spawn((
        Name::new("cabinet"),
        Mesh3d(meshes.add(Cuboid::new(cab_w, cab_h, cab_d))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.02, 0.018, 0.016),
            perceptual_roughness: 0.42,
            clearcoat: 0.9,
            clearcoat_perceptual_roughness: 0.08,
            ..default()
        })),
        Transform::from_xyz(0.0, FLOOR_Y + cab_h * 0.5, 0.0),
    ));
    // Thin dark stand mat under the glass.
    commands.spawn((
        Name::new("tank mat"),
        Mesh3d(meshes.add(Cuboid::new(
            2.0 * (HALF_W + GLASS_T) + 0.004,
            0.004,
            2.0 * (HALF_D + GLASS_T) + 0.004,
        ))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.02, 0.02, 0.02),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, CABINET_TOP - 0.001, 0.0),
    ));

    // --- The dark room: just a matte floor, the rest is the dim environment ---
    commands.spawn((
        Name::new("floor"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(14.0, 14.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.010, 0.009, 0.008),
            perceptual_roughness: 0.92,
            reflectance: 0.15,
            ..default()
        })),
        Transform::from_xyz(0.0, FLOOR_Y, 0.0),
    ));
}

/// Vertical gradient, deep navy at the bottom to a slightly lighter teal-blue at the top.
fn backdrop_gradient() -> Image {
    let h = 256u32;
    let mut data = Vec::with_capacity((h * 4) as usize);
    for y in 0..h {
        // y = 0 is the top row of the image.
        let t = 1.0 - y as f32 / (h - 1) as f32;
        let t = t * t * (3.0 - 2.0 * t);
        let bottom = Vec3::new(0.0015, 0.0035, 0.0065);
        let top = Vec3::new(0.010, 0.030, 0.050);
        let c = bottom.lerp(top, t);
        let s = Color::linear_rgb(c.x, c.y, c.z).to_srgba();
        data.extend_from_slice(&[
            (s.red * 255.0).round() as u8,
            (s.green * 255.0).round() as u8,
            (s.blue * 255.0).round() as u8,
            255,
        ]);
    }
    let mut image = Image::new(
        Extent3d {
            width: 1,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}

//! Sand bed: displaced height field with a visible cross-section behind the glass.

use bevy::{image::CompressedImageFormatSupport, pbr::ParallaxMappingMethod, prelude::*};

use super::sand_height;
use crate::{
    meshgen::MeshBuilder,
    tank::{CORNER_R, HALF_D, HALF_W},
    textures::{TexKind, TextureLibrary},
};

/// World size covered by one repetition of the sand texture.
const TILE: f32 = 0.42;

pub fn spawn_sand(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    images: Res<Assets<Image>>,
    support: Option<Res<CompressedImageFormatSupport>>,
    mut textures: ResMut<TextureLibrary>,
) {
    let mut tex = |name: &str, kind| textures.load(&images, support.as_deref(), &format!("textures/sand/sand_{name}.jpg"), kind);
    // Poly Haven displacement is a height map (white = high): `Depth` inverts it.
    let depth = tex("disp", TexKind::Depth);
    let arm = tex("arm", TexKind::Data);

    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.90, 0.74),
        base_color_texture: Some(tex("diff", TexKind::Color)),
        normal_map_texture: Some(tex("nor_gl", TexKind::Normal)),
        metallic_roughness_texture: Some(arm.clone()),
        occlusion_texture: Some(arm),
        depth_map: Some(depth),
        parallax_depth_scale: 0.035,
        parallax_mapping_method: ParallaxMappingMethod::Relief { max_steps: 4 },
        max_parallax_layer_count: 12.0,
        metallic: 0.0,
        perceptual_roughness: 1.0,
        reflectance: 0.35,
        ..default()
    });

    let mut mesh = sand_mesh().build();
    if let Err(e) = mesh.generate_tangents() {
        warn!("sand tangents: {e}");
    }
    commands.spawn((
        Name::new("sand"),
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
    ));
}

/// Pull a point inside the rounded rectangle of the tank floor (slightly inset).
fn clamp_to_footprint(p: Vec2, inset: f32) -> Vec2 {
    let (hw, hd, r) = (HALF_W - inset, HALF_D - inset, CORNER_R - inset);
    let mut q = vec2(p.x.clamp(-hw, hw), p.y.clamp(-hd, hd));
    let c = vec2(q.x.signum() * (hw - r), q.y.signum() * (hd - r));
    if q.x.abs() > hw - r && q.y.abs() > hd - r {
        let d = q - c;
        if d.length() > r {
            q = c + d.normalize() * r;
        }
    }
    q
}

fn sand_mesh() -> MeshBuilder {
    let (nx, nz) = (220usize, 96usize);
    let inset = 0.0006;
    let mut b = MeshBuilder::default();

    let mut grid = vec![0u32; (nx + 1) * (nz + 1)];
    let mut pts = vec![Vec3::ZERO; (nx + 1) * (nz + 1)];
    for j in 0..=nz {
        for i in 0..=nx {
            let x = -HALF_W + 2.0 * HALF_W * i as f32 / nx as f32;
            let z = -HALF_D + 2.0 * HALF_D * j as f32 / nz as f32;
            let q = clamp_to_footprint(vec2(x, z), inset);
            let p = vec3(q.x, sand_height(q.x, q.y), q.y);
            pts[j * (nx + 1) + i] = p;
            grid[j * (nx + 1) + i] = b.vertex(p, Vec3::Y, vec2(p.x, p.z) / TILE);
        }
    }
    for j in 0..nz {
        for i in 0..nx {
            let a = grid[j * (nx + 1) + i];
            let bb = grid[j * (nx + 1) + i + 1];
            let c = grid[(j + 1) * (nx + 1) + i + 1];
            let d = grid[(j + 1) * (nx + 1) + i];
            b.quad(a, d, c, bb);
        }
    }
    b.compute_smooth_normals();

    // Cross-section ("skirt") along the glass, visible from the outside.
    let mut ring: Vec<Vec3> = Vec::new();
    for i in 0..=nx {
        ring.push(pts[i]);
    }
    for j in 1..=nz {
        ring.push(pts[j * (nx + 1) + nx]);
    }
    for i in (0..nx).rev() {
        ring.push(pts[nz * (nx + 1) + i]);
    }
    for j in (1..nz).rev() {
        ring.push(pts[j * (nx + 1)]);
    }
    ring.dedup_by(|a, b| a.distance(*b) < 1e-5);
    let mut s = 0.0;
    let mut prev: Option<(u32, u32)> = None;
    let first = b.len();
    for k in 0..=ring.len() {
        let p = ring[k % ring.len()];
        let pn = ring[(k + 1) % ring.len()];
        let pp = ring[(k + ring.len() - 1) % ring.len()];
        if k > 0 {
            s += p.distance(pp);
        }
        let tangent = (pn - pp).with_y(0.0).normalize_or(Vec3::X);
        // Outward normal: the footprint centre is at the origin.
        let mut n = tangent.cross(Vec3::Y);
        if n.dot(p.with_y(0.0)) < 0.0 {
            n = -n;
        }
        let top = b.vertex(p, n, vec2(s / TILE, p.y / TILE));
        let bottom = b.vertex(p.with_y(-0.002), n, vec2(s / TILE, 0.0));
        if let Some((pt, pb)) = prev {
            b.quad(pb, bottom, top, pt);
        }
        prev = Some((top, bottom));
    }
    let _ = first;
    b.orient_to_normals();
    b
}

//! Procedural aquatic plants: vallisneria ribbons, red/green stem plants,
//! hairgrass tufts and sea anemones. Everything sways in the vertex shader.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use rand::{RngExt, SeedableRng, rngs::StdRng};

use super::{PlantMaterial, SwayExt, SwayParams, sand_height};
use crate::{
    meshgen::MeshBuilder,
    sdf::{DecorSdf, Grid},
    tank::{HALF_D, HALF_W, WATER_Y},
};

/// Keep plant geometry away from the glass.
fn keep_inside(p: Vec3, margin: f32) -> Vec3 {
    vec3(
        p.x.clamp(-HALF_W + margin, HALF_W - margin),
        p.y,
        p.z.clamp(-HALF_D + margin, HALF_D - margin),
    )
}

// Sway amplitudes (the vertex shader's `SwayParams`).
const VALL_SWAY: f32 = 0.075;
const STEM_SWAY: f32 = 0.03;
const GRASS_SWAY: f32 = 0.012;
const ANEMONE_SWAY: f32 = 0.007;

const NAMES: [&str; 4] = ["vallisneria", "stem plants", "hairgrass", "anemones"];

/// Which of the plant meshes an entity shows.
#[derive(Component)]
pub struct Plant(usize);

pub fn spawn_plants(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PlantMaterial>>,
) {
    let mut leaf_material = |amplitude: f32, frequency: f32, mode: f32, transmission: f32| {
        materials.add(PlantMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.7,
                reflectance: 0.22,
                double_sided: true,
                cull_mode: None,
                diffuse_transmission: transmission,
                ..default()
            },
            extension: SwayExt {
                sway: SwayParams {
                    amplitude,
                    frequency,
                    water_y: WATER_Y - 0.004,
                    mode,
                },
            },
        })
    };
    let vall_mat = leaf_material(VALL_SWAY, 0.85, 0.0, 0.45);
    let stem_mat = leaf_material(STEM_SWAY, 0.75, 0.0, 0.35);
    let grass_mat = leaf_material(GRASS_SWAY, 1.1, 0.0, 0.3);
    let anemone_mat = materials.add(PlantMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.35,
            reflectance: 0.45,
            diffuse_transmission: 0.55,
            // Anemones fluoresce faintly under the blue light.
            emissive: LinearRgba::rgb(0.035, 0.006, 0.03),
            double_sided: true,
            cull_mode: None,
            ..default()
        },
        extension: SwayExt {
            sway: SwayParams {
                amplitude: ANEMONE_SWAY,
                frequency: 0.9,
                water_y: WATER_Y - 0.004,
                mode: 1.0,
            },
        },
    });
    // The rocks aren't known yet (their scans load in the background): these
    // are replaced by `fit_plants`, behind the start-up curtain.
    let (built, _) = plant_meshes(None);
    for (i, (b, mat)) in built.into_iter().zip([vall_mat, stem_mat, grass_mat, anemone_mat]).enumerate() {
        commands.spawn((Name::new(NAMES[i]), Plant(i), Mesh3d(meshes.add(b.build())), MeshMaterial3d(mat)));
    }
}

/// Once the decor's distance field has the rocks: the plants again, none
/// growing into a stone.
pub fn fit_plants(
    sdf: Res<DecorSdf>,
    mut done: Local<bool>,
    mut meshes: ResMut<Assets<Mesh>>,
    plants: Query<(&Plant, &Mesh3d)>,
) {
    if *done || !sdf.complete || plants.is_empty() {
        return;
    }
    *done = true;
    let (built, fit) = plant_meshes(Some(&sdf));
    let mut built = built.map(Some);
    for (plant, mesh) in &plants {
        if let Some(b) = built[plant.0].take() {
            let _ = meshes.insert(mesh.id(), b.build());
        }
    }
    let counts: Vec<String> = (0..4).map(|k| format!("{} {}/{}", NAMES[k], fit.moved[k], fit.dropped[k])).collect();
    info!("plants moved clear of the rocks / left out: {}", counts.join(", "));
}

/// Plants moved clear of a rock, and left out for lack of room, per kind.
#[derive(Default)]
struct Fit {
    moved: [u32; 4],
    dropped: [u32; 4],
    kind: usize,
}

/// Every plant mesh. With the decor's field, a plant that would grow into a
/// rock (at rest or swaying) is moved to the nearest free spot, same shape.
fn plant_meshes(sdf: Option<&Grid>) -> ([MeshBuilder; 4], Fit) {
    let mut rng = StdRng::seed_from_u64(0xA0_0A);
    let mut fit = Fit::default();

    // --- Vallisneria: tall ribbons in the back, some reaching the surface ---
    let mut vall = MeshBuilder::default();
    let clusters = [
        (vec2(-0.63, -0.21), 14),
        (vec2(-0.52, -0.25), 12),
        (vec2(-0.40, -0.26), 9),
        (vec2(0.60, -0.20), 14),
        (vec2(0.47, -0.25), 12),
        (vec2(-0.05, -0.27), 8),
        (vec2(0.66, -0.05), 7),
        (vec2(-0.66, -0.02), 7),
    ];
    for (c, n) in clusters {
        for _ in 0..n {
            let base = c + vec2(rng.random_range(-0.035..0.035), rng.random_range(-0.025..0.025));
            let len = rng.random_range(0.30..0.72);
            place(&mut vall, &mut rng, sdf, &mut fit, base, VALL_SWAY, 0.04, |_| true, |b, at, r| {
                vallisneria_blade(b, at, len, r)
            });
        }
    }

    // --- Stem plants (Rotala-like): green below, orange-red crowns ---
    fit.kind = 1;
    let mut stems = MeshBuilder::default();
    let stem_groups = [
        (vec2(-0.58, -0.10), 22, 0.30, true),
        (vec2(0.60, 0.00), 16, 0.24, false),
        (vec2(-0.12, -0.21), 16, 0.33, true),
        (vec2(0.26, -0.24), 14, 0.36, false),
    ];
    for (c, n, h, red) in stem_groups {
        for _ in 0..n {
            let base = c + vec2(rng.random_range(-0.04..0.04), rng.random_range(-0.03..0.03));
            let height = h * rng.random_range(0.65..1.15);
            place(&mut stems, &mut rng, sdf, &mut fit, base, STEM_SWAY, 0.05, |_| true, |b, at, r| {
                stem_plant(b, at, height, red, r)
            });
        }
    }

    // --- Hairgrass tufts in the foreground and around the stones ---
    fit.kind = 2;
    let mut grass = MeshBuilder::default();
    // Keep the sand path clear.
    let off_path = |p: Vec2| {
        let path_x = 0.06 + 0.10 * (1.0 - (p.y + 0.3) / 0.6).powf(1.5) + 0.03 * (p.y * 9.0).sin();
        (p.x - path_x).abs() >= 0.09
    };
    let mut placed = 0;
    while placed < 70 {
        let p = vec2(rng.random_range(-0.66..0.66), rng.random_range(-0.1..0.27));
        // ...and the very front mostly clear.
        if !off_path(p) || (p.y > 0.2 && rng.random_bool(0.6)) {
            continue;
        }
        place(&mut grass, &mut rng, sdf, &mut fit, p, GRASS_SWAY, 0.08, off_path, hairgrass_tuft);
        placed += 1;
    }

    // --- Sea anemones ---
    fit.kind = 3;
    let mut anemones = MeshBuilder::default();
    for (at, scale) in [(vec2(0.02, 0.105), 1.0), (vec2(0.45, 0.14), 0.8), (vec2(-0.36, 0.02), 0.7)] {
        place(&mut anemones, &mut rng, sdf, &mut fit, at, ANEMONE_SWAY, 0.12, |_| true, |b, at, r| {
            anemone(b, at, scale, r)
        });
    }
    ([vall, stems, grass, anemones], fit)
}

/// Grows one plant with `grow` at `at`; if it would reach into a rock, the
/// same plant is slid (following the sand) to the nearest free spot within
/// `reach` where `allowed`, or left out. The random stream is the same either
/// way: every other plant keeps its shape and place.
#[allow(clippy::too_many_arguments)]
fn place(
    out: &mut MeshBuilder,
    rng: &mut StdRng,
    sdf: Option<&Grid>,
    fit: &mut Fit,
    at: Vec2,
    sway: f32,
    reach: f32,
    allowed: impl Fn(Vec2) -> bool,
    grow: impl Fn(&mut MeshBuilder, Vec2, &mut StdRng),
) {
    let mut b = MeshBuilder::default();
    grow(&mut b, at, rng);
    let Some(sdf) = sdf else {
        out.append(b);
        return;
    };
    let rings = (reach / 0.005).round() as u32;
    let spots = std::iter::once(at).chain((1..=rings).flat_map(|k| {
        let r = k as f32 * 0.005;
        (0..16).map(move |a| at + Vec2::from_angle(a as f32 * TAU / 16.0 + k as f32 * 0.4) * r)
    }));
    let rest = b.positions.clone();
    for (i, spot) in spots.enumerate() {
        if i > 0 && !(allowed(spot) && spot.x.abs() < HALF_W - 0.03 && spot.y.abs() < HALF_D - 0.03) {
            continue;
        }
        let shift = vec3(spot.x - at.x, sand_height(spot.x, spot.y) - sand_height(at.x, at.y), spot.y - at.y);
        for (p, r) in b.positions.iter_mut().zip(&rest) {
            *p = (Vec3::from(*r) + shift).to_array();
        }
        if clear_of_rocks(&b, sdf, sway) {
            if i > 0 {
                fit.moved[fit.kind] += 1;
            }
            out.append(b);
            return;
        }
    }
    fit.dropped[fit.kind] += 1;
}

/// No vertex in (or brushing, as it sways) a rock. The field also holds the
/// glass, the surface and the sand: a vertex only counts when something nearer
/// than those is there, and the roots under the sand don't count.
fn clear_of_rocks(b: &MeshBuilder, sdf: &Grid, sway: f32) -> bool {
    b.positions.iter().zip(&b.uvs_b).all(|(p, uv)| {
        let v = Vec3::from(*p);
        let sand = sand_height(v.x, v.z);
        if v.y < sand + 0.003 {
            return true;
        }
        let tank = (HALF_W - v.x.abs()).min(HALF_D - v.z.abs()).min(WATER_Y - v.y).min(v.y - sand);
        let d = sdf.distance(v);
        // `uv.x` is the vertex's flex: how far the shader sways it (capped:
        // the long ribbons' tips also rise as they bend).
        let room = 0.003 + (1.3 * sway * uv[0]).min(0.04);
        d >= room || d > tank - 0.002
    })
}

fn lerp3(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a + (b - a) * t
}

fn push_color(b: &mut MeshBuilder, c: Vec3) {
    b.colors.push([c.x, c.y, c.z, 1.0]);
}

/// One ribbon leaf growing from `base`, bending over when it reaches the surface.
fn vallisneria_blade(b: &mut MeshBuilder, base: Vec2, len: f32, rng: &mut StdRng) {
    let y0 = sand_height(base.x, base.y) - 0.01;
    let mut pos = vec3(base.x, y0, base.y);
    let lean_angle = rng.random_range(0.0..TAU);
    // Lean towards the tank centre (front), not into the glass.
    let to_center = vec3(-base.x, 0.0, 0.25 - base.y).normalize_or(Vec3::Z);
    let lean = (vec3(lean_angle.cos(), 0.0, lean_angle.sin()) * 0.6 + to_center).normalize();
    let mut dir = (Vec3::Y + lean * rng.random_range(0.05..0.25)).normalize();
    let width = rng.random_range(0.006..0.010);
    let twist_total = rng.random_range(-2.5..2.5);
    let phase = rng.random_range(0.0..TAU);
    let segs = 30;
    let ds = len / segs as f32;
    let hue = rng.random_range(-1.0..1.0f32);
    let old = rng.random_bool(0.12);
    let dark = vec3(0.02, 0.10, 0.012);
    let mid = vec3(0.09 + 0.03 * hue, 0.38, 0.03);
    let tip = if old {
        vec3(0.32, 0.30, 0.08)
    } else {
        vec3(0.24 + 0.05 * hue, 0.58, 0.05)
    };
    let side0 = dir.cross(lean).normalize_or(Vec3::X);
    for s in 0..=segs {
        let t = s as f32 / segs as f32;
        let q = Quat::from_axis_angle(dir, twist_total * t);
        let side = (q * side0 - dir * dir.dot(q * side0)).normalize_or(Vec3::X);
        let normal = dir.cross(side).normalize_or(Vec3::Z);
        // Rounded tip.
        let w = width * (1.0 - 0.15 * t) * (1.0 - ((t - 0.9) / 0.1).clamp(0.0, 1.0).powi(2));
        let color = if t < 0.5 {
            lerp3(dark, mid, t * 2.0)
        } else {
            lerp3(mid, tip, (t - 0.5) * 2.0)
        };
        let flex = t.powf(1.5) * (len / 0.55);
        for k in [-0.5f32, 0.5] {
            b.vertex(pos + side * (w * k), normal, vec2(k + 0.5, t));
            b.uvs_b.push([flex, phase]);
            push_color(b, color);
        }
        if s > 0 {
            let i = b.len() - 4;
            b.quad(i, i + 1, i + 3, i + 2);
        }
        // Grow: bend with the lean, flatten out below the surface, stay off the glass.
        pos = keep_inside(pos + dir * ds, 0.018);
        let surface = WATER_Y - 0.012;
        dir = (dir + lean * (0.9 * ds / 0.1) * (0.12 + 0.2 * t)).normalize();
        if pos.y > surface - 0.03 {
            let k = ((pos.y - (surface - 0.03)) / 0.03).clamp(0.0, 1.0);
            dir = (dir.with_y(dir.y * (1.0 - k)) + lean * k * 0.5).normalize();
        }
        pos.y = pos.y.min(surface);
    }
}

/// A leafy stem with opposite leaf pairs.
fn stem_plant(b: &mut MeshBuilder, base: Vec2, height: f32, red: bool, rng: &mut StdRng) {
    let y0 = sand_height(base.x, base.y) - 0.008;
    let lean = vec3(rng.random_range(-0.25..0.25), 1.0, rng.random_range(-0.2..0.3)).normalize();
    let phase = rng.random_range(0.0..TAU);
    let curve = vec3(rng.random_range(-0.1..0.1), 0.0, rng.random_range(-0.1..0.1));
    let at = |t: f32| vec3(base.x, y0, base.y) + lean * (height * t) + curve * (t * t * height);
    let green = vec3(0.045, 0.27, 0.025);
    let top = if red {
        vec3(0.78, 0.10, 0.04)
    } else {
        vec3(0.26, 0.58, 0.03)
    };
    let flex_of = |y: f32| ((y - y0) / 0.35).clamp(0.0, 1.0).powf(1.4) * 0.8;

    // Stem: two crossed thin strips.
    let segs = 8;
    for axis in [Vec3::X, Vec3::Z] {
        for s in 0..=segs {
            let t = s as f32 / segs as f32;
            let p = at(t);
            let c = lerp3(green * 0.8, lerp3(green, top, 0.5), t);
            for k in [-0.5f32, 0.5] {
                b.vertex(p + axis * (0.0022 * k), axis.cross(Vec3::Y), vec2(k + 0.5, t));
                b.uvs_b.push([flex_of(p.y), phase]);
                push_color(b, c);
            }
            if s > 0 {
                let i = b.len() - 4;
                b.quad(i, i + 1, i + 3, i + 2);
            }
        }
    }

    // Leaves.
    let spacing = 0.0085;
    let nodes = (height / spacing) as usize;
    let mut rot = rng.random_range(0.0..PI);
    for n in 1..nodes {
        let t = n as f32 / nodes as f32;
        let p = at(t);
        rot += PI * 0.5;
        // Leaves get smaller and more colourful towards the crown.
        let size = 0.021 * (1.0 - 0.45 * t) * rng.random_range(0.8..1.15);
        let color_t = ((t - 0.45) / 0.55).clamp(0.0, 1.0);
        let color = lerp3(green, top, color_t.powf(0.8)) * rng.random_range(0.85..1.15);
        for side in [0.0, PI] {
            let a = rot + side;
            let out = vec3(a.cos(), 0.0, a.sin());
            // Leaves point up-and-out, more upright near the crown.
            let up_angle = 0.5 + 0.6 * t;
            let dir = (out * up_angle.cos() + Vec3::Y * up_angle.sin()).normalize();
            let width_dir = out.cross(Vec3::Y).normalize();
            let normal = dir.cross(width_dir).normalize();
            let flex = flex_of(p.y);
            let len = size;
            let w = size * 0.28;
            // Leaf: flat 5-vertex diamond (slight midrib bend via the normals only).
            let v0 = b.vertex(p, normal, vec2(0.5, 0.0));
            let v1 = b.vertex(p + dir * (len * 0.35) + width_dir * w, normal + width_dir * 0.25, vec2(0.0, 0.35));
            let v2 = b.vertex(p + dir * len, normal, vec2(0.5, 1.0));
            let v3 = b.vertex(p + dir * (len * 0.35) - width_dir * w, normal - width_dir * 0.25, vec2(1.0, 0.35));
            let v4 = b.vertex(p + dir * (len * 0.45), normal, vec2(0.5, 0.45));
            for _ in 0..5 {
                b.uvs_b.push([flex, phase]);
                push_color(b, color);
            }
            b.tri(v0, v1, v4);
            b.tri(v1, v2, v4);
            b.tri(v2, v3, v4);
            b.tri(v3, v0, v4);
        }
    }
}

/// A small tuft of fine grass blades.
fn hairgrass_tuft(b: &mut MeshBuilder, center: Vec2, rng: &mut StdRng) {
    let blades = rng.random_range(14..26);
    for _ in 0..blades {
        let base = center + vec2(rng.random_range(-0.012..0.012), rng.random_range(-0.012..0.012));
        let y0 = sand_height(base.x, base.y) - 0.003;
        let len = rng.random_range(0.025..0.07);
        let a = rng.random_range(0.0..TAU);
        let splay = rng.random_range(0.1..0.55);
        let dir = (Vec3::Y + vec3(a.cos(), 0.0, a.sin()) * splay).normalize();
        let bend = vec3(a.cos(), 0.0, a.sin()) * rng.random_range(0.2..0.6);
        let side = dir.cross(Vec3::Y).normalize_or(Vec3::X);
        let normal = side.cross(dir).normalize();
        let phase = rng.random_range(0.0..TAU);
        let segs = 4;
        let tint = rng.random_range(0.8..1.2);
        for s in 0..=segs {
            let t = s as f32 / segs as f32;
            let p = vec3(base.x, y0, base.y) + dir * (len * t) + bend * (len * t * t * 0.5);
            let w = 0.0014 * (1.0 - t * 0.85);
            let c = lerp3(vec3(0.03, 0.17, 0.018), vec3(0.24, 0.55, 0.04), t) * tint;
            for k in [-0.5f32, 0.5] {
                b.vertex(p + side * (w * k), normal, vec2(k + 0.5, t));
                b.uvs_b.push([t.powf(1.5) * (len / 0.05), phase]);
                push_color(b, c);
            }
            if s > 0 {
                let i = b.len() - 4;
                b.quad(i, i + 1, i + 3, i + 2);
            }
        }
    }
}

/// A sea anemone: short column, oral disc and rings of tapered tentacles.
fn anemone(b: &mut MeshBuilder, at: Vec2, scale: f32, rng: &mut StdRng) {
    let y0 = sand_height(at.x, at.y) - 0.004;
    let origin = vec3(at.x, y0, at.y);
    let col_h = 0.026 * scale;
    let r_base = 0.016 * scale;
    let r_top = 0.022 * scale;
    let hue = rng.random_range(0.0..1.0f32);
    let body = lerp3(vec3(0.62, 0.16, 0.08), vec3(0.45, 0.10, 0.36), hue);
    let tent = lerp3(vec3(1.0, 0.22, 0.50), vec3(0.62, 0.20, 0.95), hue);
    let tip = vec3(1.0, 0.80, 0.95);
    let phase0 = rng.random_range(0.0..TAU);

    // Column (open cylinder, flared).
    let sides = 20;
    let rings = 5;
    let start = b.len();
    for r in 0..=rings {
        let t = r as f32 / rings as f32;
        let rad = r_base + (r_top - r_base) * t * t;
        for s in 0..=sides {
            let a = s as f32 / sides as f32 * TAU;
            let d = vec3(a.cos(), 0.0, a.sin());
            let bulge = 1.0 + 0.06 * (a * 3.0 + phase0).sin();
            b.vertex(origin + d * (rad * bulge) + Vec3::Y * (col_h * t), d, vec2(s as f32 / sides as f32, t));
            b.uvs_b.push([0.0, phase0]);
            push_color(b, body * (0.7 + 0.3 * t));
        }
    }
    for r in 0..rings {
        for s in 0..sides {
            let i = start + r * (sides + 1) + s;
            let j = i + sides + 1;
            b.quad(i, i + 1, j + 1, j);
        }
    }
    // Oral disc.
    let disc_c = b.vertex(origin + Vec3::Y * (col_h - 0.002 * scale), Vec3::Y, vec2(0.5, 0.5));
    b.uvs_b.push([0.0, phase0]);
    push_color(b, body * 1.2);
    let ring_start = b.len();
    for s in 0..=sides {
        let a = s as f32 / sides as f32 * TAU;
        b.vertex(origin + vec3(a.cos() * r_top, col_h, a.sin() * r_top), Vec3::Y, vec2(0.5, 0.5));
        b.uvs_b.push([0.0, phase0]);
        push_color(b, body);
    }
    for s in 0..sides {
        b.tri(disc_c, ring_start + s + 1, ring_start + s);
    }

    // Tentacles.
    let count = (70.0 * scale) as usize + 20;
    for k in 0..count {
        let ring = rng.random_range(0.35..1.0f32);
        let a = k as f32 / count as f32 * TAU * 3.0 + rng.random_range(-0.2..0.2);
        let root = origin + vec3(a.cos() * r_top * ring, col_h, a.sin() * r_top * ring);
        let out = vec3(a.cos(), 0.0, a.sin());
        let len = rng.random_range(0.028..0.05) * scale * (1.2 - 0.4 * ring);
        let tilt = 0.25 + ring * 0.9;
        let dir0 = (Vec3::Y * tilt.cos() * 1.3 + out * tilt.sin()).normalize();
        let droop = rng.random_range(0.2..0.8);
        let phase = rng.random_range(0.0..TAU);
        let segs = 7;
        let sides_t = 5;
        let base_index = b.len();
        for s in 0..=segs {
            let t = s as f32 / segs as f32;
            let d = (dir0 + out * (droop * t) - Vec3::Y * (droop * 0.6 * t * t)).normalize();
            let p = root + (dir0 + out * (droop * t * 0.5) - Vec3::Y * (droop * 0.2 * t * t)) * (len * t);
            let rad = 0.0022 * scale * (1.0 - 0.7 * t) + 0.0004;
            let u = d.cross(Vec3::Y).normalize_or(Vec3::X);
            let v = u.cross(d).normalize();
            let c = if t > 0.8 {
                lerp3(tent, tip, (t - 0.8) / 0.2)
            } else {
                tent * (0.75 + 0.3 * t)
            };
            for j in 0..=sides_t {
                let ang = j as f32 / sides_t as f32 * TAU;
                let n = u * ang.cos() + v * ang.sin();
                b.vertex(p + n * rad, n, vec2(j as f32 / sides_t as f32, t));
                b.uvs_b.push([t.powf(1.2), phase]);
                push_color(b, c);
            }
        }
        for s in 0..segs {
            for j in 0..sides_t {
                let i = base_index + s * (sides_t + 1) + j;
                let n = i + sides_t + 1;
                b.quad(i, i + 1, n + 1, n);
            }
        }
    }
}

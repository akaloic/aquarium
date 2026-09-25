//! Procedural meshes of the bottom dwellers. All built around the origin, +Y up,
//! head towards -Z. UV_1 tags the animated parts (see critter_anim.wgsl):
//! x = part (0 rigid, 5 flatfish, 6 starfish arm, 7 eye), y = phase index +
//! fraction along the part.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::meshgen::MeshBuilder;

fn v(b: &mut MeshBuilder, p: Vec3, n: Vec3, uv: Vec2, part: f32, param: f32, c: Vec3) -> u32 {
    let i = b.vertex(p, n, uv);
    b.uvs_b.push([part, param]);
    b.colors.push([c.x, c.y, c.z, 1.0]);
    i
}

/// Ellipsoid-like blob on a lat/long grid (flattened underneath).
#[allow(clippy::too_many_arguments)]
fn blob(
    b: &mut MeshBuilder,
    center: Vec3,
    radii: Vec3,
    flatten_bottom: f32,
    part: f32,
    param: f32,
    color: impl Fn(Vec3) -> Vec3,
    rings: u32,
    sectors: u32,
) {
    let start = b.len();
    for r in 0..=rings {
        let lat = PI * r as f32 / rings as f32;
        for s in 0..=sectors {
            let lon = TAU * s as f32 / sectors as f32;
            let n = Vec3::new(lat.sin() * lon.cos(), lat.cos(), lat.sin() * lon.sin());
            let mut q = n * radii;
            if q.y < 0.0 {
                q.y *= flatten_bottom;
            }
            let uv = Vec2::new(0.5 + q.x / (2.0 * radii.x), 0.5 + q.z / (2.0 * radii.z));
            v(b, center + q, (n / radii).normalize(), uv, part, param, color(n));
        }
    }
    for r in 0..rings {
        for s in 0..sectors {
            let i = start + r * (sectors + 1) + s;
            let j = i + sectors + 1;
            b.quad(i, j, j + 1, i + 1);
        }
    }
}

/// Tapered tube along a polyline (legs, antennae, stalks).
#[allow(clippy::too_many_arguments)]
fn tube(
    b: &mut MeshBuilder,
    points: &[Vec3],
    r0: f32,
    r1: f32,
    part: f32,
    phase_index: f32,
    color: Vec3,
    sides: u32,
) {
    let n = points.len();
    let start = b.len();
    for (i, &p) in points.iter().enumerate() {
        let t = i as f32 / (n - 1) as f32;
        let dir = if i + 1 < n { points[i + 1] - p } else { p - points[i - 1] }.normalize_or(Vec3::Z);
        let side = dir.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = side.cross(dir).normalize();
        let r = r0 + (r1 - r0) * t;
        for s in 0..=sides {
            let a = TAU * s as f32 / sides as f32;
            let nrm = side * a.cos() + up * a.sin();
            v(b, p + nrm * r, nrm, Vec2::new(s as f32 / sides as f32, t), part, phase_index + t.min(0.999), color);
        }
    }
    for i in 0..n as u32 - 1 {
        for s in 0..sides {
            let a = start + i * (sides + 1) + s;
            let c = a + sides + 1;
            b.quad(a, a + 1, c + 1, c);
        }
    }
}

fn finish(mut b: MeshBuilder) -> Mesh {
    b.orient_to_normals();
    b.build()
}

// ---------------------------------------------------------------------------
// Crab: carapace with eyes on stalks; limbs are separate instanced segments.
// ---------------------------------------------------------------------------

pub fn crab_body(width: f32, shell: Vec3, belly: Vec3) -> Mesh {
    let mut b = MeshBuilder::default();
    let radii = Vec3::new(width * 0.5, width * 0.15, width * 0.4);
    blob(&mut b, Vec3::ZERO, radii, 0.5, 0.0, 0.0, |n| if n.y > -0.1 { shell } else { belly }, 14, 32);
    // Squarish carapace (a grapsid crab): push the outline towards a rounded
    // rectangle, slightly narrower at the back, with a raised brow.
    for p in b.positions.iter_mut() {
        let q = Vec3::from(*p);
        let (nx, nz) = (q.x / radii.x, q.z / radii.z);
        let r = (nx * nx + nz * nz).sqrt().max(1e-5);
        let sq = (nx.abs().powf(4.0) + nz.abs().powf(4.0)).powf(0.25).max(1e-5);
        let k = r / sq;
        let back = 1.0 - 0.12 * (q.z / radii.z).max(0.0);
        let brow = 1.0 + 0.25 * (-(q.z / radii.z) - 0.6).max(0.0) * (q.y > 0.0) as u32 as f32;
        *p = [q.x * k * back, q.y * brow, q.z * k];
    }
    // Eye stalks and eyes at the front edge.
    for sx in [-1.0f32, 1.0] {
        let base = Vec3::new(sx * width * 0.14, width * 0.1, -width * 0.36);
        let tip = base + Vec3::new(sx * width * 0.04, width * 0.12, -width * 0.04);
        tube(&mut b, &[base, tip], width * 0.035, width * 0.03, 0.0, 0.0, shell, 8);
        blob(&mut b, tip, Vec3::splat(width * 0.045), 1.0, 7.0, 0.0, |_| Vec3::splat(0.02), 6, 10);
    }
    // Mouth parts.
    blob(
        &mut b,
        Vec3::new(0.0, -width * 0.02, -width * 0.36),
        Vec3::new(width * 0.12, width * 0.05, width * 0.05),
        1.0,
        0.0,
        0.0,
        |_| belly,
        6,
        12,
    );
    finish(b)
}

/// Unit limb segment along +Z (length 1, base radius 1, tip radius 0.7).
pub fn limb_segment(color: Vec3, tip_color: Vec3) -> Mesh {
    let mut b = MeshBuilder::default();
    let sides = 8;
    let rings = 4;
    let start = b.len();
    for r in 0..=rings {
        let t = r as f32 / rings as f32;
        let rad = 1.0 - 0.3 * t + 0.15 * (t * PI).sin();
        for s in 0..=sides {
            let a = TAU * s as f32 / sides as f32;
            let n = Vec3::new(a.cos(), a.sin(), 0.0);
            v(&mut b, n * rad + Vec3::Z * t, n, Vec2::new(s as f32 / sides as f32, t), 0.0, 0.0, color.lerp(tip_color, t * t));
        }
    }
    for r in 0..rings {
        for s in 0..sides {
            let i = start + r * (sides + 1) + s;
            let j = i + sides + 1;
            b.quad(i, i + 1, j + 1, j);
        }
    }
    // Rounded end caps.
    blob(&mut b, Vec3::Z, Vec3::new(0.75, 0.75, 0.2), 1.0, 0.0, 0.0, |_| tip_color, 4, 8);
    blob(&mut b, Vec3::ZERO, Vec3::new(1.0, 1.0, 0.2), 1.0, 0.0, 0.0, |_| color, 4, 8);
    finish(b)
}

/// Claw (propodus + two fingers) along +Z, unit length.
pub fn claw(color: Vec3, tip: Vec3) -> Mesh {
    let mut b = MeshBuilder::default();
    blob(&mut b, Vec3::new(0.0, 0.0, 0.3), Vec3::new(0.22, 0.17, 0.32), 1.0, 0.0, 0.0, |_| color, 8, 14);
    // Fixed finger and dactyl, slightly apart.
    for (y, w) in [(-0.05f32, 0.09f32), (0.08, 0.08)] {
        let pts = [
            Vec3::new(0.0, y, 0.55),
            Vec3::new(0.02, y * 1.3, 0.8),
            Vec3::new(0.0, y * 0.4, 1.0),
        ];
        let n = pts.len();
        let _ = n;
        tube(&mut b, &pts, w, 0.015, 0.0, 0.0, color.lerp(tip, 0.6), 6);
    }
    finish(b)
}

// ---------------------------------------------------------------------------
// Flatfish: oval body with a continuous fin fringe, both eyes on top.
// ---------------------------------------------------------------------------

pub fn flatfish(length: f32) -> Mesh {
    let mut b = MeshBuilder::default();
    let (a, c) = (length * 0.28, length * 0.5);
    let rings = 12;
    let sectors = 40;
    for side in [1.0f32, -1.0] {
        let start = b.len();
        let h0 = if side > 0.0 { 0.06 * length } else { -0.03 * length };
        v(&mut b, Vec3::Y * h0, Vec3::Y * side, Vec2::splat(0.5), 5.0, 0.0, Vec3::ONE);
        for ri in 1..=rings {
            let t = ri as f32 / rings as f32;
            // Beyond t = 0.8: the thin fin fringe.
            let fringe = ((t - 0.8) / 0.2).clamp(0.0, 1.0);
            for s in 0..sectors {
                let ang = TAU * s as f32 / sectors as f32;
                let x = ang.cos() * a * t * (1.0 + 0.25 * fringe);
                let z = ang.sin() * c * t;
                let body = (1.0 - (t / 0.8).min(1.0).powi(2)).max(0.0).sqrt();
                let y = (if side > 0.0 { 0.06 } else { -0.03 }) * length * body + side * 0.002 * length * (1.0 - fringe);
                let uv = Vec2::new(0.5 + x / length, 0.5 + z / length);
                v(&mut b, Vec3::new(x, y, z), Vec3::Y * side, uv, 5.0, fringe.min(0.999), Vec3::ONE);
            }
        }
        for s in 0..sectors as u32 {
            let p = start + 1 + s;
            let q = start + 1 + (s + 1) % sectors as u32;
            if side > 0.0 {
                b.tri(start, q, p);
            } else {
                b.tri(start, p, q);
            }
        }
        for ri in 0..rings as u32 - 1 {
            for s in 0..sectors as u32 {
                let p = start + 1 + ri * sectors as u32 + s;
                let q = start + 1 + ri * sectors as u32 + (s + 1) % sectors as u32;
                if side > 0.0 {
                    b.quad(p, q, q + sectors as u32, p + sectors as u32);
                } else {
                    b.quad(p, p + sectors as u32, q + sectors as u32, q);
                }
            }
        }
    }
    // Tail fin.
    let tail_start = b.len();
    for (i, t) in [0.0f32, 1.0].into_iter().enumerate() {
        for s in [-1.0f32, 1.0] {
            let p = Vec3::new(s * length * (0.05 + 0.12 * t), 0.0, c * (0.98 + 0.28 * t));
            v(&mut b, p, Vec3::Y, Vec2::new(0.5 + p.x / length, 0.5 + p.z / length), 5.0, 0.5 + 0.49 * i as f32, Vec3::ONE);
        }
    }
    b.quad(tail_start, tail_start + 1, tail_start + 3, tail_start + 2);
    b.quad(tail_start, tail_start + 2, tail_start + 3, tail_start + 1);
    // Eyes, both on the upper side, near the head.
    for (x, z) in [(-0.05f32, -0.3f32), (0.02, -0.35)] {
        blob(&mut b, Vec3::new(x * length, 0.055 * length, z * length), Vec3::splat(0.03 * length), 1.0, 7.0, 0.0, |_| Vec3::ONE, 5, 8);
    }
    finish(b)
}

// ---------------------------------------------------------------------------
// Starfish: five arms, domed top, flat underside.
// ---------------------------------------------------------------------------

pub fn starfish(radius: f32) -> Mesh {
    let mut b = MeshBuilder::default();
    let rings = 14;
    let sectors = 120;
    let outline = |a: f32| radius * (0.2 + 0.8 * (2.5 * a).cos().abs().powf(3.5));
    for side in [1.0f32, -1.0] {
        let start = b.len();
        v(&mut b, Vec3::Y * side * 0.2 * radius * if side > 0.0 { 1.0 } else { 0.25 }, Vec3::Y * side, Vec2::new(0.0, 0.0), 6.0, 0.0, Vec3::ONE);
        for ri in 1..=rings {
            let t = ri as f32 / rings as f32;
            for s in 0..sectors {
                let a = TAU * s as f32 / sectors as f32;
                let rb = outline(a);
                let rr = rb * t;
                let arm = rb / radius;
                let dome = (1.0 - t * t).max(0.0).sqrt() * (0.35 + 0.65 * (1.0 - rr / radius));
                let y = side * 0.2 * radius * dome * if side > 0.0 { 1.0 } else { 0.25 } * (0.5 + 0.5 * arm);
                let p = Vec3::new(a.cos() * rr, y, a.sin() * rr);
                let n = Vec3::new(p.x * 0.4 / radius, side, p.z * 0.4 / radius).normalize();
                v(&mut b, p, n, Vec2::new(a / TAU, rr / radius), 6.0, (rr / radius).min(0.999), Vec3::ONE);
            }
        }
        for s in 0..sectors as u32 {
            let p = start + 1 + s;
            let q = start + 1 + (s + 1) % sectors as u32;
            if side > 0.0 {
                b.tri(start, q, p);
            } else {
                b.tri(start, p, q);
            }
        }
        for ri in 0..rings as u32 - 1 {
            for s in 0..sectors as u32 {
                let p = start + 1 + ri * sectors as u32 + s;
                let q = start + 1 + ri * sectors as u32 + (s + 1) % sectors as u32;
                if side > 0.0 {
                    b.quad(p, q, q + sectors as u32, p + sectors as u32);
                } else {
                    b.quad(p, p + sectors as u32, q + sectors as u32, q);
                }
            }
        }
    }
    finish(b)
}

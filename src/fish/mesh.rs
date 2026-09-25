//! Procedural fish meshes: a lofted body, membrane fins and glossy eyes.
//!
//! Local frame: the nose points to -Z (Bevy's forward), Y is up, X is the
//! fish's left/right. UV_1 carries (part, extra): part 0 = body, 1 = fin,
//! 2 = eye, 3 = pectoral fin (flutters).
//! Body UV: (along 0 nose → 1 tail base, height 0 belly → 1 back).
//! Fin UV: (base → edge, along the fin).

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::meshgen::MeshBuilder;

#[derive(Clone, Copy)]
pub struct Fin {
    /// Start / end along the body (0 nose, 1 tail base).
    pub from: f32,
    pub to: f32,
    /// Height relative to the fish length.
    pub height: f32,
    /// How far the tip is swept back (relative to the fin base length).
    pub sweep: f32,
    /// 0 = rounded, 1 = pointed at the front (triangle), 2 = two lobes.
    pub shape: u8,
}

#[derive(Clone, Copy)]
pub struct Shape {
    /// Total length including the tail fin (m).
    pub length: f32,
    /// Fraction of the length taken by the body (the rest is the tail fin).
    pub body: f32,
    /// Max half-height and half-width relative to the length.
    pub height: f32,
    pub width: f32,
    /// Where the body is the tallest (0..1 along the body).
    pub hump: f32,
    /// Head bluntness (smaller = blunter).
    pub head: f32,
    /// Half-height at the tail base, relative to the length.
    pub peduncle: f32,
    pub dorsal: Fin,
    pub anal: Fin,
    /// Tail fin: length and half-height relative to the length, fork depth
    /// (negative = rounded), extra length of the lobe tips (streamers).
    pub caudal_len: f32,
    pub caudal_h: f32,
    pub fork: f32,
    pub streamers: f32,
    pub pectoral: f32,
    /// Long pelvic filaments (angelfish), relative to the length. 0 = none.
    pub pelvic: f32,
    pub eye: f32,
}

impl Shape {
    fn body_len(&self) -> f32 {
        self.length * self.body
    }
    fn nose_z(&self) -> f32 {
        -0.42 * self.length
    }
    fn z_at(&self, s: f32) -> f32 {
        self.nose_z() + s * self.body_len()
    }
    /// Normalised height profile along the body.
    fn profile(&self, s: f32) -> f32 {
        let ped = self.peduncle / self.height;
        if s < self.hump {
            (0.5 * PI * s / self.hump).sin().powf(self.head)
        } else {
            let t = (s - self.hump) / (1.0 - self.hump);
            1.0 - t.powf(1.5) * (1.0 - ped)
        }
    }
    pub fn half_height(&self, s: f32) -> f32 {
        self.length * self.height * self.profile(s)
    }
    pub fn half_width(&self, s: f32) -> f32 {
        let p = self.profile(s);
        // The tail base is much thinner sideways than tall.
        let taper = 1.0 - 0.6 * ((s - 0.6) / 0.4).clamp(0.0, 1.0);
        self.length * self.width * p * taper
    }
    /// Vertical position of the body axis (slightly arched back).
    pub fn center_y(&self, s: f32) -> f32 {
        self.length * self.height * 0.06 * (PI * s).sin()
    }
}

/// Builds the whole fish (body + fins + eyes) as one mesh.
pub fn fish_mesh(shape: &Shape) -> Mesh {
    let mut b = MeshBuilder::default();
    body(&mut b, shape);
    dorsal_or_anal(&mut b, shape, &shape.dorsal, 1.0);
    dorsal_or_anal(&mut b, shape, &shape.anal, -1.0);
    caudal(&mut b, shape);
    pectorals(&mut b, shape);
    if shape.pelvic > 0.0 {
        pelvic_filaments(&mut b, shape);
    }
    eyes(&mut b, shape);
    b.build()
}

fn part(b: &mut MeshBuilder, kind: f32) {
    b.uvs_b.push([kind, 0.0]);
}

fn body(b: &mut MeshBuilder, shape: &Shape) {
    let rings = 40;
    let seg = 24;
    let start = b.len();
    for r in 0..=rings {
        // Denser rings near the nose, where the shape changes fastest.
        let s = (r as f32 / rings as f32).powf(1.25);
        let z = shape.z_at(s);
        let (h, w, yc) = (shape.half_height(s), shape.half_width(s), shape.center_y(s));
        for k in 0..=seg {
            let th = -0.5 * PI + TAU * k as f32 / seg as f32;
            let p = vec3(w * th.cos(), yc + h * th.sin(), z);
            b.vertex(p, Vec3::X, vec2(s, 0.5 + 0.5 * th.sin()));
            part(b, 0.0);
        }
    }
    let first_index = b.indices.len();
    for r in 0..rings {
        for k in 0..seg {
            let i = start + r * (seg + 1) + k;
            let j = i + seg + 1;
            b.quad(i, j, j + 1, i + 1);
        }
    }
    // Smooth normals for the body only, then make sure they point outwards.
    let tris: Vec<u32> = b.indices[first_index..].to_vec();
    let mut acc = vec![Vec3::ZERO; (b.len() - start) as usize];
    for t in tris.chunks_exact(3) {
        let pa = Vec3::from(b.positions[t[0] as usize]);
        let pb = Vec3::from(b.positions[t[1] as usize]);
        let pc = Vec3::from(b.positions[t[2] as usize]);
        let n = (pb - pa).cross(pc - pa);
        for &v in t {
            acc[(v - start) as usize] += n;
        }
    }
    // Probe: the vertex on the right flank mid-body must face +X.
    let probe = ((rings / 2) * (seg + 1) + seg / 2) as usize;
    let flip = acc[probe].x < 0.0;
    for (i, n) in acc.into_iter().enumerate() {
        let n = if flip { -n } else { n };
        b.normals[start as usize + i] = n.normalize_or(Vec3::X).to_array();
    }
    if flip {
        for t in b.indices[first_index..].chunks_exact_mut(3) {
            t.swap(1, 2);
        }
    }
}

/// Membrane fin on the back (side = 1) or belly (side = -1).
fn dorsal_or_anal(b: &mut MeshBuilder, shape: &Shape, fin: &Fin, side: f32) {
    if fin.to <= fin.from || fin.height <= 0.0 {
        return;
    }
    let cols = 14;
    let rows = 5;
    let start = b.len();
    let base_len = shape.body_len() * (fin.to - fin.from);
    for c in 0..=cols {
        let t = c as f32 / cols as f32;
        let s = fin.from + t * (fin.to - fin.from);
        let z = shape.z_at(s);
        // Root sits slightly inside the body so there is no gap.
        let root_y = shape.center_y(s) + side * shape.half_height(s) * 0.9;
        let profile = match fin.shape {
            // Triangle: rises fast, long trailing slope.
            1 => (t / 0.3).min(1.0).powf(0.8) * (1.0 - ((t - 0.3) / 0.7).max(0.0) * 0.25),
            // Two lobes (spiny front part + soft rear part).
            2 => {
                let a = (PI * (t / 0.5).min(1.0)).sin().max(0.0);
                let c2 = (PI * ((t - 0.45) / 0.55).clamp(0.0, 1.0)).sin() * 1.15;
                a.max(c2) * 0.85 + 0.1
            }
            _ => (PI * t).sin().powf(0.6),
        };
        let height = fin.height * shape.length * profile;
        for r in 0..=rows {
            let f = r as f32 / rows as f32;
            let y = root_y + side * height * f;
            let z = z + fin.sweep * base_len * f * f * (0.3 + 0.7 * t);
            b.vertex(vec3(0.0, y, z), Vec3::X, vec2(f, t));
            part(b, 1.0);
        }
    }
    grid(b, start, cols, rows);
}

fn caudal(b: &mut MeshBuilder, shape: &Shape) {
    let cols = 10;
    let rows = 12;
    let start = b.len();
    let z0 = shape.z_at(1.0) - 0.01 * shape.length;
    let y0 = shape.center_y(1.0);
    let len = shape.caudal_len * shape.length;
    let ped = shape.peduncle * shape.length;
    let tip_h = shape.caudal_h * shape.length;
    for c in 0..=cols {
        let t = c as f32 / cols as f32;
        for r in 0..=rows {
            let j = r as f32 / rows as f32 * 2.0 - 1.0;
            let a = j.abs();
            // Rear edge: forked (lobes longer) or rounded (centre longer).
            let reach = if shape.fork >= 0.0 {
                1.0 - shape.fork * (1.0 - a).powf(1.3) + shape.streamers * a.powf(6.0)
            } else {
                1.0 + shape.fork * a * a
            };
            let half = ped * 0.85 + (tip_h - ped * 0.85) * t.powf(0.6);
            let y = y0 + j * half;
            let z = z0 + len * t * reach;
            b.vertex(vec3(0.0, y, z), Vec3::X, vec2(t, 0.5 + 0.5 * j));
            part(b, 1.0);
        }
    }
    grid(b, start, cols, rows);
}

fn pectorals(b: &mut MeshBuilder, shape: &Shape) {
    let s = 0.3;
    let size = shape.pectoral * shape.length;
    let z = shape.z_at(s);
    let y = shape.center_y(s) - shape.half_height(s) * 0.25;
    for side in [-1.0f32, 1.0] {
        let root = vec3(side * shape.half_width(s) * 0.9, y, z);
        let out = vec3(side * 0.55, -0.15, 0.8).normalize();
        let up = Vec3::Y;
        let n = out.cross(up).normalize() * side;
        let start = b.len();
        let cols = 4;
        for c in 0..=cols {
            let t = c as f32 / cols as f32;
            let w = size * 0.45 * (PI * (0.15 + 0.85 * t)).sin();
            let p = root + out * (size * t);
            for (k, dy) in [(0.0, -0.5f32), (1.0, 0.5)] {
                b.vertex(p + up * (w * dy), n, vec2(t, k));
                part(b, 3.0);
            }
        }
        for c in 0..cols {
            let i = start + c * 2;
            b.quad(i, i + 2, i + 3, i + 1);
        }
    }
}

fn pelvic_filaments(b: &mut MeshBuilder, shape: &Shape) {
    let s = 0.36;
    let z = shape.z_at(s);
    let y = shape.center_y(s) - shape.half_height(s) * 0.92;
    let len = shape.pelvic * shape.length;
    for side in [-1.0f32, 1.0] {
        let start = b.len();
        let segs = 8;
        for k in 0..=segs {
            let t = k as f32 / segs as f32;
            let p = vec3(side * 0.002, y - len * t, z + len * 0.45 * t * t);
            let w = 0.0022 * (1.0 - 0.7 * t);
            for (u, dz) in [(0.0, -1.0f32), (1.0, 1.0)] {
                b.vertex(p + Vec3::Z * (w * dz), Vec3::X, vec2(u, t));
                part(b, 1.0);
            }
        }
        for k in 0..segs {
            let i = start + k * 2;
            b.quad(i, i + 2, i + 3, i + 1);
        }
    }
}

fn eyes(b: &mut MeshBuilder, shape: &Shape) {
    let s = 0.1;
    let r = shape.eye * shape.length;
    let z = shape.z_at(s) + r * 0.2;
    let y = shape.center_y(s) + shape.half_height(s) * 0.3;
    for side in [-1.0f32, 1.0] {
        let c = vec3(side * (shape.half_width(s) * 0.8 + r * 0.25), y, z);
        let axis = Vec3::X * side;
        let (rings, seg) = (8, 12);
        let start = b.len();
        for i in 0..=rings {
            let phi = PI * i as f32 / rings as f32;
            for j in 0..=seg {
                let th = TAU * j as f32 / seg as f32;
                let n = vec3(phi.cos() * side, phi.sin() * th.cos(), phi.sin() * th.sin());
                // uv.x: 1 at the centre of the pupil, 0 at the back of the eyeball.
                b.vertex(c + n * r, n, vec2(0.5 + 0.5 * n.dot(axis), 0.0));
                part(b, 2.0);
            }
        }
        for i in 0..rings {
            for j in 0..seg {
                let a = start + i * (seg + 1) + j;
                let d = a + seg + 1;
                b.quad(a, d, d + 1, a + 1);
            }
        }
        // Outward winding.
        let end = b.indices.len();
        let first = end - rings as usize * seg as usize * 6;
        let t = &b.indices[first..first + 3];
        let (pa, pb, pc) = (
            Vec3::from(b.positions[t[0] as usize]),
            Vec3::from(b.positions[t[1] as usize]),
            Vec3::from(b.positions[t[2] as usize]),
        );
        let g = (pb - pa).cross(pc - pa);
        if g.dot(pa - c) < 0.0 {
            for t in b.indices[first..end].chunks_exact_mut(3) {
                t.swap(1, 2);
            }
        }
    }
}

fn grid(b: &mut MeshBuilder, start: u32, cols: u32, rows: u32) {
    for c in 0..cols {
        for r in 0..rows {
            let i = start + c * (rows + 1) + r;
            let j = i + rows + 1;
            b.quad(i, j, j + 1, i + 1);
        }
    }
}

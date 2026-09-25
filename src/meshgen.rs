//! Small helpers to build procedural meshes.

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

#[derive(Default)]
pub struct MeshBuilder {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Optional second UV channel, used by the sway shaders as (flex, phase).
    pub uvs_b: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl MeshBuilder {
    pub fn vertex(&mut self, p: Vec3, n: Vec3, uv: Vec2) -> u32 {
        self.positions.push(p.to_array());
        self.normals.push(n.normalize_or(Vec3::Y).to_array());
        self.uvs.push(uv.to_array());
        (self.positions.len() - 1) as u32
    }

    pub fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend_from_slice(&[a, b, c]);
    }

    /// Quad a-b-c-d, counter-clockwise when seen from the front.
    pub fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.indices.extend_from_slice(&[a, b, c, a, c, d]);
    }

    pub fn len(&self) -> u32 {
        self.positions.len() as u32
    }

    /// Adds another builder's geometry to this one.
    pub fn append(&mut self, other: MeshBuilder) {
        let base = self.len();
        self.positions.extend(other.positions);
        self.normals.extend(other.normals);
        self.uvs.extend(other.uvs);
        self.uvs_b.extend(other.uvs_b);
        self.colors.extend(other.colors);
        self.indices.extend(other.indices.into_iter().map(|i| i + base));
    }

    /// Recompute smooth normals from the triangles (area weighted).
    pub fn compute_smooth_normals(&mut self) {
        let mut acc = vec![Vec3::ZERO; self.positions.len()];
        for t in self.indices.chunks_exact(3) {
            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let pa = Vec3::from(self.positions[a]);
            let pb = Vec3::from(self.positions[b]);
            let pc = Vec3::from(self.positions[c]);
            let n = (pb - pa).cross(pc - pa);
            acc[a] += n;
            acc[b] += n;
            acc[c] += n;
        }
        for (dst, n) in self.normals.iter_mut().zip(acc) {
            *dst = n.normalize_or(Vec3::Y).to_array();
        }
    }

    /// Flip triangles whose geometric normal disagrees with their vertex normals.
    pub fn orient_to_normals(&mut self) {
        for t in self.indices.chunks_exact_mut(3) {
            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let pa = Vec3::from(self.positions[a]);
            let g = (Vec3::from(self.positions[b]) - pa).cross(Vec3::from(self.positions[c]) - pa);
            let vn = Vec3::from(self.normals[a]) + Vec3::from(self.normals[b]) + Vec3::from(self.normals[c]);
            if g.dot(vn) < 0.0 {
                t.swap(1, 2);
            }
        }
    }

    pub fn build(self) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        if !self.uvs_b.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, self.uvs_b);
        }
        if !self.colors.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        }
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

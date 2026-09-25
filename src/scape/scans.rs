//! Photogrammetry scans (glTF, Poly Haven), read without Bevy's glTF loader:
//! that one also decodes every texture a file references (ours go through
//! textures.rs) and keeps a CPU copy of every mesh. Here each primitive is
//!
//! - simplified (meshoptimizer), within an absolute error well under a pixel
//!   at the wallpaper framing (0.2 mm; a pixel covers ~0.33 mm at the glass):
//!   the boulders were scanned for close-ups (98 k triangles for a 20 cm
//!   stone) and their normal maps carry the fine relief anyway. Fewer
//!   triangles in every pass that draws them (depth prepass, main pass, spot
//!   and point-light shadow maps) and a faster distance-field bake;
//! - reordered for the GPU's vertex cache, given tangents (normal mapping),
//!   uploaded once and dropped on the CPU side (`RENDER_WORLD`);
//! - handed to the decor distance field as plain triangles, freed once baked.
//!
//! `AQ_NO_SIMPLIFY=1` keeps the full scans (A/B comparisons).

use std::{
    collections::HashMap,
    sync::Arc,
    time::Instant,
};

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, block_on, poll_once},
};

pub struct ScansPlugin;

impl Plugin for ScansPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScanLibrary>().add_systems(PreUpdate, finish_jobs);
    }
}

/// Largest acceptable deviation of a simplified surface (m, in the tank).
const WORLD_ERROR: f32 = 0.0002;

/// Triangles of a scan in its own coordinates (for the distance field).
pub struct Shape {
    pub positions: Vec<Vec3>,
    pub indices: Vec<u32>,
}

/// Which scan primitive a decor entity shows.
#[derive(Component, Clone, PartialEq, Eq, Hash)]
pub struct ScanPart {
    pub model: &'static str,
    pub mesh: usize,
}

type Job = Task<Result<(Mesh, Shape, usize), String>>;

#[derive(Resource, Default)]
pub struct ScanLibrary {
    meshes: HashMap<ScanPart, Handle<Mesh>>,
    shapes: HashMap<ScanPart, Arc<Shape>>,
    jobs: Vec<(ScanPart, Job)>,
    started: Option<Instant>,
    triangles: (usize, usize),
}

impl ScanLibrary {
    /// Starts reading a scan primitive (once per part), simplified for the
    /// largest `scale` it is shown at. The handle is usable at once.
    pub fn load(&mut self, meshes: &Assets<Mesh>, part: ScanPart, scale: f32) -> Handle<Mesh> {
        if let Some(h) = self.meshes.get(&part) {
            return h.clone();
        }
        let handle = meshes.reserve_handle();
        // `AQ_SCAN_ERROR=mm` overrides the tolerance (measurements).
        let world = std::env::var("AQ_SCAN_ERROR").ok().and_then(|v| v.parse::<f32>().ok()).map_or(WORLD_ERROR, |mm| mm / 1000.0);
        let error = if std::env::var("AQ_NO_SIMPLIFY").is_ok() { 0.0 } else { world / scale.max(1e-3) };
        let (model, index) = (part.model, part.mesh);
        self.started.get_or_insert_with(Instant::now);
        let task = AsyncComputeTaskPool::get().spawn(async move { read(model, index, error) });
        self.jobs.push((part.clone(), task));
        self.meshes.insert(part, handle.clone());
        handle
    }

    /// Every scan is loaded.
    pub fn ready(&self) -> bool {
        self.jobs.is_empty() && !self.meshes.is_empty()
    }

    pub fn shape(&self, part: &ScanPart) -> Option<&Shape> {
        self.shapes.get(part).map(|s| s.as_ref())
    }

    /// The distance field has what it needs: free the CPU triangles.
    pub fn release_shapes(&mut self) {
        self.shapes.clear();
    }
}

fn finish_jobs(mut lib: ResMut<ScanLibrary>, mut meshes: ResMut<Assets<Mesh>>) {
    if lib.jobs.is_empty() {
        return;
    }
    let mut done = Vec::new();
    lib.jobs.retain_mut(|(part, task)| match block_on(poll_once(task)) {
        None => true,
        Some(result) => {
            done.push((part.clone(), result));
            false
        }
    });
    for (part, result) in done {
        match result {
            Ok((mesh, shape, before)) => {
                lib.triangles.0 += before;
                lib.triangles.1 += shape.indices.len() / 3;
                if let Some(handle) = lib.meshes.get(&part) {
                    let _ = meshes.insert(handle.id(), mesh);
                }
                lib.shapes.insert(part, Arc::new(shape));
            }
            Err(e) => error!("scan {} #{}: {e}", part.model, part.mesh),
        }
    }
    if lib.jobs.is_empty() {
        let secs = lib.started.take().map_or(0.0, |t| t.elapsed().as_secs_f32());
        let (before, after) = lib.triangles;
        info!(
            "scans: {} primitives, {before} -> {after} triangles ({:.0}%) in {:.2} s",
            lib.meshes.len(),
            after as f32 * 100.0 / before.max(1) as f32,
            secs
        );
    }
}

/// Reads one primitive, simplifies it within `error` (mesh units; 0 = keep
/// everything) and builds the GPU-only mesh.
fn read(model: &str, mesh_index: usize, error: f32) -> Result<(Mesh, Shape, usize), String> {
    let dir = crate::assets_dir().join("models").join(model);
    let json = std::fs::read(dir.join(format!("{model}.gltf"))).map_err(|e| e.to_string())?;
    let gltf = gltf::Gltf::from_slice(&json).map_err(|e| e.to_string())?;
    let buffers = gltf
        .buffers()
        .map(|b| match b.source() {
            gltf::buffer::Source::Uri(uri) => std::fs::read(dir.join(uri)).map_err(|e| e.to_string()),
            gltf::buffer::Source::Bin => gltf.blob.clone().ok_or_else(|| "no binary chunk".to_string()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let primitive = gltf
        .meshes()
        .nth(mesh_index)
        .and_then(|m| m.primitives().next())
        .ok_or("no such mesh")?;
    let reader = primitive.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
    let positions: Vec<[f32; 3]> = reader.read_positions().ok_or("no positions")?.collect();
    let normals: Vec<[f32; 3]> = reader.read_normals().ok_or("no normals")?.collect();
    let uvs: Vec<[f32; 2]> = reader.read_tex_coords(0).ok_or("no UVs")?.into_f32().collect();
    let indices: Vec<u32> = reader.read_indices().ok_or("no indices")?.into_u32().collect();
    let before = indices.len() / 3;

    let kept = if error > 0.0 {
        meshopt::simplify_decoder(&indices, &positions, 0, error, meshopt::SimplifyOptions::ErrorAbsolute, None)
    } else {
        indices
    };
    let mut kept = meshopt::optimize_vertex_cache(&kept, positions.len());
    // Only the vertices still referenced, in first-use order.
    let mut remap = vec![u32::MAX; positions.len()];
    let (mut p, mut n, mut t) = (Vec::new(), Vec::new(), Vec::new());
    for i in kept.iter_mut() {
        let old = *i as usize;
        if remap[old] == u32::MAX {
            remap[old] = p.len() as u32;
            p.push(positions[old]);
            n.push(normals[old]);
            t.push(uvs[old]);
        }
        *i = remap[old];
    }
    let shape = Shape {
        positions: p.iter().map(|v| Vec3::from(*v)).collect(),
        indices: kept.clone(),
    };
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, t)
        .with_inserted_indices(Indices::U32(kept));
    mesh.generate_tangents().map_err(|e| e.to_string())?;
    Ok((mesh, shape, before))
}

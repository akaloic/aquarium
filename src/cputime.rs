//! Debug (`AQ_CPU=1`): wall time of the main-world and render-world updates.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use bevy::{prelude::*, render::{Render, RenderApp, RenderSystems}};

static MAIN_NS: AtomicU64 = AtomicU64::new(0);
static RENDER_NS: AtomicU64 = AtomicU64::new(0);
static FRAMES: AtomicU64 = AtomicU64::new(0);
static STEER_NS: AtomicU64 = AtomicU64::new(0);

pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("AQ_CPU").is_ok())
}

pub fn add_steer(d: std::time::Duration) {
    STEER_NS.fetch_add(d.as_nanos() as u64, Ordering::Relaxed);
}

#[derive(Resource)]
struct Start(Instant);

pub struct CpuTimePlugin;

impl Plugin for CpuTimePlugin {
    fn build(&self, app: &mut App) {
        // `AQ_MEM=1`: what the CPU-side copies of textures and meshes weigh.
        if std::env::var("AQ_MEM").is_ok() {
            app.add_systems(Last, report_memory);
        }
        if std::env::var("AQ_CPU").is_err() {
            return;
        }
        app.insert_resource(Start(Instant::now()))
            .add_systems(First, |mut s: ResMut<Start>| s.0 = Instant::now())
            .add_systems(Last, |s: Res<Start>, real: Res<Time<Real>>, mut n: Local<u32>, mut dts: Local<Vec<f32>>| {
                MAIN_NS.fetch_add(s.0.elapsed().as_nanos() as u64, Ordering::Relaxed);
                let f = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
                dts.push(real.delta_secs() * 1000.0);
                *n += 1;
                if *n % 90 == 0 {
                    let m = MAIN_NS.swap(0, Ordering::Relaxed) as f64 / 90.0 / 1e6;
                    let r = RENDER_NS.swap(0, Ordering::Relaxed) as f64 / 90.0 / 1e6;
                    let st = STEER_NS.swap(0, Ordering::Relaxed) as f64 / 90.0 / 1e3;
                    // Frame pacing: mean, median, 95th percentile, and the share of
                    // frames that missed a 60 Hz refresh.
                    dts.sort_by(f32::total_cmp);
                    let mean = dts.iter().sum::<f32>() / dts.len() as f32;
                    let late = dts.iter().filter(|d| **d > 18.0).count() * 100 / dts.len();
                    info!(
                        "cpu: main world {m:.2} ms, render world {r:.2} ms per update, fish steering {st:.1} µs (frame {f}); \
                         frames {:.0} fps, median {:.1} ms, p95 {:.1} ms, {late}% > 18 ms",
                        1000.0 / mean,
                        dts[dts.len() / 2],
                        dts[dts.len() * 95 / 100]
                    );
                    dts.clear();
                }
            });
        let render = app.sub_app_mut(RenderApp);
        render
            .insert_resource(Start(Instant::now()))
            .add_systems(Render, (|mut s: ResMut<Start>| s.0 = Instant::now()).in_set(RenderSystems::ExtractCommands))
            .add_systems(Render, (|s: Res<Start>| {
                RENDER_NS.fetch_add(s.0.elapsed().as_nanos() as u64, Ordering::Relaxed);
            }).in_set(RenderSystems::Cleanup));
    }
}


fn report_memory(
    mut frame: Local<u32>,
    images: Res<Assets<Image>>,
    meshes: Res<Assets<Mesh>>,
    entities: Query<(Option<&Name>, Has<Mesh3d>)>,
) {
    *frame += 1;
    if *frame != 300 {
        return;
    }
    let mut by_name: std::collections::HashMap<String, (usize, usize)> = Default::default();
    for (name, has_mesh) in &entities {
        let e = by_name.entry(name.map_or("<unnamed>".into(), |n| n.as_str().to_string())).or_default();
        e.0 += 1;
        e.1 += has_mesh as usize;
    }
    let mut v: Vec<_> = by_name.into_iter().collect();
    v.sort_by(|a, b| b.1.0.cmp(&a.1.0));
    info!("entities: {} ({} with a mesh)", entities.iter().count(), entities.iter().filter(|e| e.1).count());
    for (name, (n, m)) in v.iter().take(10) {
        info!("  {n:5} {name} ({m} with a mesh)");
    }
    let mut img_bytes = 0usize;
    let mut img_count = 0;
    let mut biggest: Vec<(usize, String)> = Vec::new();
    for (id, image) in images.iter() {
        let n = image.data.as_ref().map_or(0, |d| d.len());
        img_bytes += n;
        img_count += 1;
        let s = image.texture_descriptor.size;
        biggest.push((n, format!("{id:?} {}x{} {:?} mips {}", s.width, s.height, image.texture_descriptor.format, image.texture_descriptor.mip_level_count)));
    }
    biggest.sort_by(|a, b| b.0.cmp(&a.0));
    let mut mesh_bytes = 0usize;
    let mut verts = 0usize;
    let mut tris = 0usize;
    let mut gpu_only = 0;
    for (_, mesh) in meshes.iter() {
        // GPU-only meshes (scans) no longer have their data here.
        if mesh.try_attributes().is_err() {
            gpu_only += 1;
            continue;
        }
        verts += mesh.count_vertices();
        mesh_bytes += mesh.get_vertex_buffer_size();
        if let Some(ix) = mesh.indices() {
            mesh_bytes += ix.len() * if matches!(ix, bevy::mesh::Indices::U16(_)) { 2 } else { 4 };
            tris += ix.len() / 3;
        }
    }
    info!(
        "memory: {img_count} images, CPU copies {:.0} MB; {} meshes ({gpu_only} GPU-only), {verts} vertices, {tris} triangles, CPU copies {:.0} MB",
        img_bytes as f64 / 1e6,
        meshes.len(),
        mesh_bytes as f64 / 1e6
    );
    for (n, name) in biggest.iter().take(12) {
        info!("  {:6.1} MB  {name}", *n as f64 / 1e6);
    }
}

//! Signed distance field of the static decor — glass, water surface, sand bed,
//! rocks, driftwood, pebbles and shell — baked once at start-up on a 1 cm grid.
//!
//! Everything that needs to know "how far is the decor, and which way is out"
//! (fish avoidance, sinking food, the bottom dwellers walking on sand and rocks)
//! does a trilinear lookup + gradient here instead of casting rays.
//!
//! The glass, surface and sand are analytic and available from the first frame;
//! the scanned meshes are added by a background task once their glTFs are loaded
//! (exact point-triangle distances in a narrow band, propagated to the whole grid,
//! signed by a flood fill from the open water).

use std::{collections::VecDeque, sync::Arc, time::Instant};

use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, block_on, poll_once},
};

use crate::{
    scape::{
        sand_height,
        scans::{ScanLibrary, ScanPart},
    },
    tank::{HALF_D, HALF_W, WATER_Y},
};

/// Grid spacing (m).
pub const CELL: f32 = 0.01;

/// Static meshes baked into the field.
#[derive(Component)]
pub struct Decor;

pub struct Grid {
    origin: Vec3,
    dims: UVec3,
    d: Vec<f32>,
    /// The scanned meshes are in (not just the analytic glass, surface, sand).
    pub complete: bool,
}

impl Grid {
    #[inline]
    fn at(&self, x: u32, y: u32, z: u32) -> f32 {
        self.d[((z * self.dims.y + y) * self.dims.x + x) as usize]
    }

    /// Trilinear distance (m), positive in the water.
    pub fn distance(&self, p: Vec3) -> f32 {
        let max = (self.dims - UVec3::ONE).as_vec3();
        let g = (p - self.origin) / CELL;
        let gc = g.clamp(Vec3::ZERO, max - Vec3::splat(1e-4));
        // Outside the grid: add the distance to it (conservative).
        let outside = (g - gc).length() * CELL;
        let i = gc.floor().as_uvec3();
        let f = gc - i.as_vec3();
        let c = |dx: u32, dy: u32, dz: u32| self.at(i.x + dx, i.y + dy, i.z + dz);
        let x00 = c(0, 0, 0) + (c(1, 0, 0) - c(0, 0, 0)) * f.x;
        let x10 = c(0, 1, 0) + (c(1, 1, 0) - c(0, 1, 0)) * f.x;
        let x01 = c(0, 0, 1) + (c(1, 0, 1) - c(0, 0, 1)) * f.x;
        let x11 = c(0, 1, 1) + (c(1, 1, 1) - c(0, 1, 1)) * f.x;
        let y0 = x00 + (x10 - x00) * f.y;
        let y1 = x01 + (x11 - x01) * f.y;
        y0 + (y1 - y0) * f.z - outside
    }

    /// Distance and outward unit normal (central differences: smooth across cells).
    pub fn sample(&self, p: Vec3) -> (f32, Vec3) {
        let h = CELL * 0.75;
        let d = self.distance(p);
        let g = Vec3::new(
            self.distance(p + Vec3::X * h) - self.distance(p - Vec3::X * h),
            self.distance(p + Vec3::Y * h) - self.distance(p - Vec3::Y * h),
            self.distance(p + Vec3::Z * h) - self.distance(p - Vec3::Z * h),
        );
        (d, g.normalize_or(Vec3::Y))
    }

    /// Moves `p` onto the surface (distance `offset` above it) along the gradient.
    pub fn project(&self, mut p: Vec3, offset: f32) -> (Vec3, Vec3) {
        let mut n = Vec3::Y;
        for _ in 0..6 {
            let (d, g) = self.sample(p);
            n = g;
            p -= g * (d - offset);
            if (d - offset).abs() < 0.0002 {
                break;
            }
        }
        (p, n)
    }

    /// Sphere tracing along a ray; returns the distance to the first surface
    /// (within half a millimetre).
    pub fn march(&self, o: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let mut t = 0.0;
        for _ in 0..128 {
            let d = self.distance(o + dir * t);
            if d < 0.0005 {
                return Some(t);
            }
            t += d.max(0.0005);
            if t > max {
                return None;
            }
        }
        None
    }

    /// The ground straight below `p`: starts a little above it, lower if that
    /// is inside a rock (under an overhang), then sphere-traces down. Exact on
    /// sand and gentle slopes, no sideways jumps between surfaces.
    pub fn drop_to_ground(&self, p: Vec3) -> Vec3 {
        for h in [0.025, 0.012, 0.005] {
            let from = p + Vec3::Y * h;
            if self.distance(from) > 0.002 {
                if let Some(t) = self.march(from, Vec3::NEG_Y, 0.12) {
                    return from - Vec3::Y * t;
                }
            }
        }
        self.project(p, 0.0).0
    }
}

#[derive(Resource, Clone)]
pub struct DecorSdf(pub Arc<Grid>);

impl std::ops::Deref for DecorSdf {
    type Target = Grid;
    fn deref(&self) -> &Grid {
        &self.0
    }
}

pub struct SdfPlugin;

impl Plugin for SdfPlugin {
    fn build(&self, app: &mut App) {
        let t0 = Instant::now();
        let layout = Layout::new();
        let grid = bake(&layout, &[]);
        info!("decor SDF: analytic part in {:.0} ms", t0.elapsed().as_secs_f32() * 1000.0);
        app.insert_resource(DecorSdf(Arc::new(grid)))
            .add_systems(Update, (start_bake, finish_bake, report_plants));
    }
}

#[derive(Resource)]
struct BakeTask(Task<(Grid, usize, f32)>);

#[derive(Resource)]
struct Baked;

/// Once every scan is loaded, bakes the full field in the background (the
/// same simplified triangles the GPU draws), then frees them.
fn start_bake(
    mut commands: Commands,
    mut frames: Local<u32>,
    started: Option<Res<BakeTask>>,
    baked: Option<Res<Baked>>,
    mut scans: ResMut<ScanLibrary>,
    decor: Query<(&ScanPart, &GlobalTransform), With<Decor>>,
) {
    if started.is_some() || baked.is_some() || decor.is_empty() {
        return;
    }
    // Wait for every scan (and a couple of frames for the transforms).
    if !scans.ready() {
        *frames = 0;
        return;
    }
    *frames += 1;
    if *frames < 3 {
        return;
    }
    let mut tris = Vec::new();
    let mut pieces: Vec<(String, Vec<[Vec3; 3]>)> = Vec::new();
    for (part, gt) in &decor {
        let Some(shape) = scans.shape(part) else {
            continue;
        };
        let affine = gt.affine();
        let world: Vec<Vec3> = shape.positions.iter().map(|p| affine.transform_point3(*p)).collect();
        let mut own = Vec::new();
        for t in shape.indices.chunks_exact(3) {
            tris.push([world[t[0] as usize], world[t[1] as usize], world[t[2] as usize]]);
            own.push([world[t[0] as usize], world[t[1] as usize], world[t[2] as usize]]);
        }
        let c = gt.translation();
        pieces.push((format!("{}#{} @({:.2},{:.2})", part.model, part.mesh, c.x, c.z), own));
    }
    if std::env::var("AQ_OVERLAPS").is_ok() {
        report_overlaps(&pieces);
    }
    scans.release_shapes();
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let t0 = Instant::now();
        let grid = bake(&Layout::new(), &tris);
        (grid, tris.len(), t0.elapsed().as_secs_f32())
    });
    commands.insert_resource(BakeTask(task));
}

fn finish_bake(mut commands: Commands, task: Option<ResMut<BakeTask>>) {
    let Some(mut task) = task else {
        return;
    };
    if let Some((grid, n, secs)) = block_on(poll_once(&mut task.0)) {
        info!("decor SDF: {n} triangles baked in {:.0} ms", secs * 1000.0);
        if let Ok(path) = std::env::var("AQ_SDF_SLICE") {
            dump_slices(&grid, &path);
        }
        // Debug: `AQ_SDF_PROBE=x,z` prints the field along a vertical line.
        if let Some(v) = std::env::var("AQ_SDF_PROBE")
            .ok()
            .map(|s| s.split(',').filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>())
            .filter(|v| v.len() == 2)
        {
            for i in 0..24 {
                let y = 0.0 + i as f32 * 0.004;
                let p = Vec3::new(v[0], y, v[1]);
                let (d, n) = grid.sample(p);
                info!("probe y={y:.3} d={:.2} mm n={n:.2} sand={:.4}", d * 1000.0, sand_height(v[0], v[1]));
            }
            let p = Vec3::new(v[0], 0.03, v[1]);
            info!("drop {:?}", grid.drop_to_ground(p));
        }
        commands.insert_resource(DecorSdf(Arc::new(grid)));
        commands.remove_resource::<BakeTask>();
        commands.insert_resource(Baked);
    }
}

// ---------------------------------------------------------------------------
// Baking
// ---------------------------------------------------------------------------

struct Layout {
    origin: Vec3,
    dims: UVec3,
}

impl Layout {
    fn new() -> Self {
        let lo = Vec3::new(-HALF_W - 0.02, -0.02, -HALF_D - 0.02);
        let hi = Vec3::new(HALF_W + 0.02, WATER_Y + 0.02, HALF_D + 0.02);
        let dims = ((hi - lo) / CELL).ceil().as_uvec3() + UVec3::ONE;
        Self { origin: lo, dims }
    }
    fn pos(&self, x: u32, y: u32, z: u32) -> Vec3 {
        self.origin + UVec3::new(x, y, z).as_vec3() * CELL
    }
    fn index(&self, x: u32, y: u32, z: u32) -> usize {
        ((z * self.dims.y + y) * self.dims.x + x) as usize
    }
}

fn bake(l: &Layout, tris: &[[Vec3; 3]]) -> Grid {
    let (nx, ny, nz) = (l.dims.x, l.dims.y, l.dims.z);
    let n = (nx * ny * nz) as usize;

    // Sand heights (and slopes) on the grid columns: the fbm is too slow per node.
    let mut sand = vec![0.0f32; (nx * nz) as usize];
    for z in 0..nz {
        for x in 0..nx {
            let p = l.pos(x, 0, z);
            sand[(z * nx + x) as usize] = sand_height(p.x, p.z);
        }
    }
    let sand_at = |x: u32, z: u32| sand[(z.min(nz - 1) * nx + x.min(nx - 1)) as usize];

    // Analytic part: glass panes, water surface, sand bed (positive in the water).
    let mut d = vec![0.0f32; n];
    for z in 0..nz {
        for x in 0..nx {
            let h = sand_at(x, z);
            let gx = (sand_at(x + 1, z) - sand_at(x.saturating_sub(1), z)) / (2.0 * CELL);
            let gz = (sand_at(x, z + 1) - sand_at(x, z.saturating_sub(1))) / (2.0 * CELL);
            let slope = (1.0 + gx * gx + gz * gz).sqrt();
            for y in 0..ny {
                let p = l.pos(x, y, z);
                let walls = (HALF_W - p.x.abs()).min(HALF_D - p.z.abs()).min(WATER_Y - p.y);
                d[l.index(x, y, z)] = walls.min((p.y - h) / slope);
            }
        }
    }
    if tris.is_empty() {
        return Grid { origin: l.origin, dims: l.dims, d, complete: false };
    }

    // --- Rocks: exact distances in a narrow band around every triangle ---
    const NONE: u32 = u32::MAX;
    let mut dist = vec![f32::INFINITY; n];
    let mut closest = vec![Vec3::ZERO; n];
    let mut tri_of = vec![NONE; n];
    let band = 1.5 * CELL;
    let max_i = (l.dims - UVec3::ONE).as_ivec3();
    for (ti, t) in tris.iter().enumerate() {
        let lo = t[0].min(t[1]).min(t[2]) - Vec3::splat(band);
        let hi = t[0].max(t[1]).max(t[2]) + Vec3::splat(band);
        let i0 = ((lo - l.origin) / CELL).ceil().as_ivec3().max(IVec3::ZERO);
        let i1 = ((hi - l.origin) / CELL).floor().as_ivec3().min(max_i);
        if i0.cmpgt(i1).any() {
            continue;
        }
        for z in i0.z..=i1.z {
            for y in i0.y..=i1.y {
                for x in i0.x..=i1.x {
                    let p = l.pos(x as u32, y as u32, z as u32);
                    let q = closest_on_triangle(p, t[0], t[1], t[2]);
                    let dd = p.distance(q);
                    let k = l.index(x as u32, y as u32, z as u32);
                    if dd < dist[k] {
                        dist[k] = dd;
                        closest[k] = q;
                        tri_of[k] = ti as u32;
                    }
                }
            }
        }
    }
    let in_band: Vec<bool> = dist.iter().map(|&v| v <= band).collect();

    // --- Propagate the closest surface point to the whole grid (two sweeps each way) ---
    let mut offsets = Vec::new();
    for dz in -1i32..=1 {
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let o = IVec3::new(dx, dy, dz);
                // "Previous" half in raster order.
                if (dz, dy, dx) < (0, 0, 0) {
                    offsets.push(o);
                }
            }
        }
    }
    let dims = l.dims.as_ivec3();
    let mut sweep = |forward: bool| {
        let coords = |i: i32, n: i32| if forward { i } else { n - 1 - i };
        for zi in 0..dims.z {
            let z = coords(zi, dims.z);
            for yi in 0..dims.y {
                let y = coords(yi, dims.y);
                for xi in 0..dims.x {
                    let x = coords(xi, dims.x);
                    let k = l.index(x as u32, y as u32, z as u32);
                    let p = l.pos(x as u32, y as u32, z as u32);
                    for o in &offsets {
                        let o = if forward { *o } else { -*o };
                        let q = IVec3::new(x, y, z) + o;
                        if q.cmplt(IVec3::ZERO).any() || q.cmpge(dims).any() {
                            continue;
                        }
                        let kq = l.index(q.x as u32, q.y as u32, q.z as u32);
                        if tri_of[kq] == NONE {
                            continue;
                        }
                        let dd = p.distance(closest[kq]);
                        if dd < dist[k] {
                            dist[k] = dd;
                            closest[k] = closest[kq];
                            tri_of[k] = tri_of[kq];
                        }
                    }
                }
            }
        }
    };
    for _ in 0..2 {
        sweep(true);
        sweep(false);
    }

    // --- Sign: flood fill the open water from the top; unreached = inside a rock ---
    // (Only the sand bounds the fill: the top layer is above the water surface.)
    let passable = |x: u32, y: u32, z: u32| {
        !in_band[l.index(x, y, z)] && l.pos(x, y, z).y - sand_at(x, z) > -0.5 * CELL
    };
    let mut outside = vec![false; n];
    let mut queue = VecDeque::new();
    for z in 0..nz {
        for x in 0..nx {
            let k = l.index(x, ny - 1, z);
            if passable(x, ny - 1, z) {
                outside[k] = true;
                queue.push_back(UVec3::new(x, ny - 1, z));
            }
        }
    }
    while let Some(c) = queue.pop_front() {
        let c = c.as_ivec3();
        for o in [IVec3::X, -IVec3::X, IVec3::Y, -IVec3::Y, IVec3::Z, -IVec3::Z] {
            let q = c + o;
            if q.cmplt(IVec3::ZERO).any() || q.cmpge(dims).any() {
                continue;
            }
            let k = l.index(q.x as u32, q.y as u32, q.z as u32);
            if !outside[k] && passable(q.x as u32, q.y as u32, q.z as u32) {
                outside[k] = true;
                queue.push_back(q.as_uvec3());
            }
        }
    }
    for z in 0..nz {
        for y in 0..ny {
            for x in 0..nx {
                let k = l.index(x, y, z);
                if tri_of[k] == NONE {
                    continue;
                }
                let below_sand = l.pos(x, y, z).y - sand_at(x, z) < -0.5 * CELL;
                let sign = if in_band[k] {
                    // Near the surface: which side of the closest triangle.
                    let t = &tris[tri_of[k] as usize];
                    let normal = (t[1] - t[0]).cross(t[2] - t[0]);
                    if (l.pos(x, y, z) - closest[k]).dot(normal) >= 0.0 { 1.0 } else { -1.0 }
                } else if outside[k] || below_sand {
                    // Under the sand the flood fill can't reach, but the sand term
                    // already makes it solid: counting it as rock would bend the
                    // field (and the sand surface) around it.
                    1.0
                } else {
                    -1.0
                };
                d[k] = d[k].min(sign * dist[k]);
            }
        }
    }
    Grid { origin: l.origin, dims: l.dims, d, complete: true }
}

/// Closest point on triangle abc to p (Ericson, Real-Time Collision Detection).
fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// Debug (`AQ_SDF_SLICE=prefix`): horizontal and vertical slices as PGM images.
fn dump_slices(grid: &Grid, prefix: &str) {
    use std::io::Write;
    let (nx, ny, nz) = (grid.dims.x, grid.dims.y, grid.dims.z);
    let shade = |v: f32| -> u8 {
        if v < 0.0 {
            40
        } else {
            (90.0 + (v * 1500.0).min(160.0) - 25.0 * ((v * 200.0).fract() < 0.1) as u8 as f32) as u8
        }
    };
    let write = |name: &str, w: u32, h: u32, f: &dyn Fn(u32, u32) -> f32| {
        let mut out = std::fs::File::create(format!("{prefix}_{name}.pgm")).unwrap();
        write!(out, "P5 {w} {h} 255\n").unwrap();
        let mut buf = Vec::with_capacity((w * h) as usize);
        for j in 0..h {
            for i in 0..w {
                buf.push(shade(f(i, j)));
            }
        }
        out.write_all(&buf).unwrap();
        // The same slice as JSON (millimetres), for the project page.
        let mm: Vec<String> = (0..h).flat_map(|j| (0..w).map(move |i| (i, j))).map(|(i, j)| format!("{:.0}", f(i, j) * 1000.0)).collect();
        std::fs::write(
            format!("{prefix}_{name}.json"),
            format!("{{\"w\":{w},\"h\":{h},\"cell_mm\":{:.0},\"d\":[{}]}}", CELL * 1000.0, mm.join(",")),
        )
        .unwrap();
    };
    // Plan view at `AQ_SDF_SLICE_Y` metres (9 cm by default), and a front section.
    let height = std::env::var("AQ_SDF_SLICE_Y").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.09);
    let y = ((height - grid.origin.y) / CELL).round().max(0.0) as u32;
    write("top", nx, nz, &|i, j| grid.at(i, y.min(ny - 1), j));
    let z = nz / 2;
    write("front", nx, ny, &|i, j| grid.at(i, ny - 1 - j, z));
}

/// Debug: how deep each scanned piece sinks into the others (not the sand).
fn report_overlaps(pieces: &[(String, Vec<[Vec3; 3]>)]) {
    let l = Layout::new();
    for (i, (name, own)) in pieces.iter().enumerate() {
        let others: Vec<[Vec3; 3]> = pieces.iter().enumerate().filter(|(j, _)| *j != i).flat_map(|(_, (_, t))| t.iter().copied()).collect();
        let grid = bake(&l, &others);
        // Deepest point of this piece in the others, moved by `shift`.
        let depth = |shift: Vec3| {
            let mut worst = 0.0f32;
            for t in own {
                for &v0 in t {
                    let v = v0 + shift;
                    let near_glass = v.x.abs() > HALF_W - 0.015 || v.z.abs() > HALF_D - 0.015 || v.y > WATER_Y - 0.015;
                    if near_glass || v.y < sand_height(v.x, v.z) + 0.003 {
                        continue;
                    }
                    worst = worst.max(-grid.distance(v));
                }
            }
            worst
        };
        let mut fix = String::new();
        if depth(Vec3::ZERO) > 0.0015 {
            'search: for r in (2..=40).step_by(2) {
                let mut best: Option<(f32, Vec3)> = None;
                for k in 0..24 {
                    let a = k as f32 * std::f32::consts::TAU / 24.0;
                    let shift = Vec3::new(a.cos(), 0.0, a.sin()) * (r as f32 / 1000.0);
                    let d = depth(shift);
                    if d <= 0.0015 && best.is_none_or(|b| d < b.0) {
                        best = Some((d, shift));
                    }
                }
                if let Some((d, shift)) = best {
                    fix = format!("; clears ({:.1} mm left) if moved by ({:+.3}, {:+.3})", d * 1000.0, shift.x, shift.z);
                    break 'search;
                }
            }
        }
        let (mut worst, mut at, mut n_in, mut n) = (0.0f32, Vec3::ZERO, 0, 0);
        for t in own {
            for &v in t {
                let near_glass = v.x.abs() > HALF_W - 0.015 || v.z.abs() > HALF_D - 0.015 || v.y > WATER_Y - 0.015;
                if near_glass || v.y < sand_height(v.x, v.z) + 0.003 {
                    continue;
                }
                n += 1;
                let d = grid.distance(v);
                if d < -0.002 {
                    n_in += 1;
                }
                if -d > worst {
                    worst = -d;
                    at = v;
                }
            }
        }
        // Which other piece is there.
        let partner = pieces
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .min_by(|a, b| {
                let da = a.1.1.iter().flat_map(|t| t.iter()).map(|p| p.distance(at)).fold(f32::MAX, f32::min);
                let db = b.1.1.iter().flat_map(|t| t.iter()).map(|p| p.distance(at)).fold(f32::MAX, f32::min);
                da.total_cmp(&db)
            })
            .map(|(_, p)| p.0.clone())
            .unwrap_or_default();
        info!(
            "overlap: {name}: {:.1} mm deep at ({:.3},{:.3},{:.3}), {:.1}% of its points > 2 mm inside, into {partner}{fix}",
            worst * 1000.0, at.x, at.y, at.z, n_in as f32 * 100.0 / n.max(1) as f32
        );
    }
}

/// Debug: plant vertices inside the rocks, per plant kind and 5 cm spot.
fn report_plants(
    sdf: Res<DecorSdf>,
    mut frames: Local<u32>,
    meshes: Res<Assets<Mesh>>,
    plants: Query<(&Name, &Mesh3d)>,
) {
    if !sdf.complete || std::env::var("AQ_OVERLAPS").is_err() {
        return;
    }
    // After `fit_plants` has placed them.
    *frames += 1;
    if *frames != 10 {
        return;
    }
    for (name, mesh) in &plants {
        if !["vallisneria", "stem plants", "hairgrass", "anemones"].contains(&name.as_str()) {
            continue;
        }
        let Some(mesh) = meshes.get(&mesh.0) else { continue };
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { continue };
        let mut spots: std::collections::BTreeMap<(i32, i32), (u32, f32)> = Default::default();
        let mut inside = 0;
        for p in pos {
            let v = Vec3::from(*p);
            let near_glass = v.x.abs() > HALF_W - 0.015 || v.z.abs() > HALF_D - 0.015 || v.y > WATER_Y - 0.015;
            if near_glass || v.y < sand_height(v.x, v.z) + 0.003 {
                continue;
            }
            let d = sdf.distance(v);
            if d < -0.002 {
                inside += 1;
                let e = spots.entry(((v.x / 0.05).floor() as i32, (v.z / 0.05).floor() as i32)).or_default();
                e.0 += 1;
                e.1 = e.1.max(-d);
            }
        }
        let list: Vec<String> = spots.iter().map(|((x, z), (n, d))| format!("({:.2},{:.2}) {n} pts {:.0} mm", *x as f32 * 0.05 + 0.025, *z as f32 * 0.05 + 0.025, d * 1000.0)).collect();
        info!("plants in rocks: {name}: {inside} of {} vertices; {}", pos.len(), list.join(", "));
    }
}

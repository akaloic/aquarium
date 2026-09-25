//! Textures, the GPU's way: JPEG -> mip chain (gamma-correct box filter) ->
//! block compression (BC7 colour and data, BC5 normals, BC4 depth) -> one
//! upload.
//!
//! - 4x less GPU memory and bandwidth than RGBA8 (8x for the depth map). BC7
//!   measures 48 dB PSNR on these scans (49.8 dB with a 17x slower preset):
//!   indistinguishable;
//! - no CPU copy is kept (`RenderAssetUsages::RENDER_WORLD`): the pixels live
//!   on the GPU only (they used to be kept twice, 483 MB on the CPU side);
//! - the compressed chain is cached on disk (`~/Library/Caches/aquarium`):
//!   later launches read ~5 MB per texture instead of decoding a JPEG,
//!   building 12 mips and compressing them.
//!
//! `AQ_TEX_RAW=1` keeps uncompressed RGBA8 (A/B comparisons).

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

use bevy::{
    asset::RenderAssetUsages,
    image::{
        CompressedImageFormatSupport, CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler,
        ImageSamplerDescriptor,
    },
    prelude::*,
    render::render_resource::{Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages},
    tasks::{AsyncComputeTaskPool, Task, block_on, poll_once},
};
use block_compression::{BC7Settings, CompressionVariant, encode::compress_rgba8};

pub struct TexturesPlugin;

impl Plugin for TexturesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TextureLibrary>().add_systems(PreUpdate, finish_jobs);
    }
}

/// Bump when the processing changes: old cache entries are then ignored.
const VERSION: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TexKind {
    /// sRGB colour (BC7).
    Color,
    /// Linear data in RGB: occlusion / roughness / metallic (BC7).
    Data,
    /// Tangent-space normal map, OpenGL convention (BC5: X and Y; the shader
    /// rebuilds Z).
    Normal,
    /// A height map, stored inverted: the depth map Bevy's parallax wants (BC4).
    Depth,
}

static PENDING: AtomicUsize = AtomicUsize::new(0);

/// Every requested texture is on its way to the GPU.
pub fn textures_ready() -> bool {
    PENDING.load(Ordering::Relaxed) == 0
}

#[derive(Resource, Default)]
pub struct TextureLibrary {
    loaded: HashMap<(String, TexKind), Handle<Image>>,
    jobs: Vec<(Handle<Image>, String, Task<Result<(Image, bool), String>>)>,
    started: Option<Instant>,
    from_cache: u32,
}

impl TextureLibrary {
    /// A texture of the assets folder, processed in the background; the handle
    /// is usable at once (the material waits for the image).
    pub fn load(
        &mut self,
        images: &Assets<Image>,
        support: Option<&CompressedImageFormatSupport>,
        path: &str,
        kind: TexKind,
    ) -> Handle<Image> {
        if let Some(h) = self.loaded.get(&(path.to_string(), kind)) {
            return h.clone();
        }
        let handle = images.reserve_handle();
        let file = crate::assets_dir().join(path);
        let bc = support.is_some_and(|s| s.0.contains(CompressedImageFormats::BC))
            && std::env::var("AQ_TEX_RAW").is_err();
        PENDING.fetch_add(1, Ordering::Relaxed);
        self.started.get_or_insert_with(Instant::now);
        let task = AsyncComputeTaskPool::get().spawn(async move { build(&file, kind, bc) });
        self.jobs.push((handle.clone(), path.to_string(), task));
        self.loaded.insert((path.to_string(), kind), handle.clone());
        handle
    }
}

fn finish_jobs(mut lib: ResMut<TextureLibrary>, mut images: ResMut<Assets<Image>>) {
    if lib.jobs.is_empty() {
        return;
    }
    let mut cached = 0;
    let jobs_before = lib.jobs.len();
    lib.jobs.retain_mut(|(handle, path, task)| {
        let Some(result) = block_on(poll_once(task)) else {
            return true;
        };
        PENDING.fetch_sub(1, Ordering::Relaxed);
        let image = match result {
            Ok((image, from_cache)) => {
                cached += from_cache as u32;
                image
            }
            Err(e) => {
                // A neutral grey rather than a material that never shows.
                error!("texture {path}: {e}");
                Image::new_fill(
                    Extent3d::default(),
                    TextureDimension::D2,
                    &[128, 128, 255, 255],
                    TextureFormat::Rgba8Unorm,
                    RenderAssetUsages::RENDER_WORLD,
                )
            }
        };
        let _ = images.insert(handle.id(), image);
        false
    });
    lib.from_cache += cached;
    if lib.jobs.is_empty() && jobs_before > 0 {
        let secs = lib.started.take().map_or(0.0, |t| t.elapsed().as_secs_f32());
        info!("textures: {} ready in {:.2} s ({} from the cache)", lib.loaded.len(), secs, lib.from_cache);
    }
}

pub fn repeat_sampler() -> ImageSamplerDescriptor {
    ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    }
}

fn format_of(kind: TexKind, bc: bool) -> TextureFormat {
    match (kind, bc) {
        (TexKind::Color, true) => TextureFormat::Bc7RgbaUnormSrgb,
        (TexKind::Data, true) => TextureFormat::Bc7RgbaUnorm,
        (TexKind::Normal, true) => TextureFormat::Bc5RgUnorm,
        (TexKind::Depth, true) => TextureFormat::Bc4RUnorm,
        (TexKind::Color, false) => TextureFormat::Rgba8UnormSrgb,
        _ => TextureFormat::Rgba8Unorm,
    }
}

fn variant_of(kind: TexKind) -> CompressionVariant {
    match kind {
        TexKind::Color | TexKind::Data => CompressionVariant::BC7(BC7Settings::opaque_ultra_fast()),
        TexKind::Normal => CompressionVariant::BC5,
        TexKind::Depth => CompressionVariant::BC4,
    }
}

fn gpu_image(size: UVec2, levels: u32, format: TextureFormat, data: Vec<u8>) -> Image {
    Image {
        data: Some(data),
        texture_descriptor: TextureDescriptor {
            label: None,
            size: Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        sampler: ImageSampler::Descriptor(repeat_sampler()),
        asset_usage: RenderAssetUsages::RENDER_WORLD,
        ..default()
    }
}

/// Decodes, filters, compresses, or reads it all from the cache. Returns the
/// image and whether it came from the cache.
fn build(file: &Path, kind: TexKind, bc: bool) -> Result<(Image, bool), String> {
    let format = format_of(kind, bc);
    let cache = cache_path(file, kind, bc);
    if let Some(image) = cache.as_deref().and_then(|c| read_cache(c, format)) {
        return Ok((image, true));
    }
    let mut rgba = image::open(file).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = rgba.dimensions();
    if kind == TexKind::Depth {
        for px in rgba.chunks_exact_mut(4) {
            px[0] = 255 - px[0];
            px[1] = 255 - px[1];
            px[2] = 255 - px[2];
        }
    }
    let levels = mip_chain(rgba.into_raw(), w, h, kind == TexKind::Color);
    let mut data = Vec::new();
    for (level, (lw, lh)) in &levels {
        if bc {
            compress(variant_of(kind), level, *lw, *lh, &mut data);
        } else {
            data.extend_from_slice(level);
        }
    }
    let image = gpu_image(UVec2::new(w, h), levels.len() as u32, format, data);
    if let (Some(path), Some(bytes)) = (cache, image.data.as_ref()) {
        write_cache(&path, UVec2::new(w, h), levels.len() as u32, bytes);
    }
    Ok((image, false))
}

/// Full chain, level 0 included, each with its size (box filter, averaged in
/// linear light for sRGB).
fn mip_chain(base: Vec<u8>, w: u32, h: u32, srgb: bool) -> Vec<(Vec<u8>, (u32, u32))> {
    let to_lin: Vec<f32> = (0..256)
        .map(|i| {
            let c = i as f32 / 255.0;
            if !srgb {
                c
            } else if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
        .collect();
    let encode = |l: f32| -> u8 {
        let c = if !srgb {
            l
        } else if l <= 0.0031308 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
    };
    let mut out = vec![(base, (w, h))];
    let (mut pw, mut ph) = (w, h);
    while pw > 1 || ph > 1 {
        let (nw, nh) = ((pw / 2).max(1), (ph / 2).max(1));
        let prev = &out.last().unwrap().0;
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(pw - 1);
                    let sy = (y * 2 + dy).min(ph - 1);
                    let i = ((sy * pw + sx) * 4) as usize;
                    for c in 0..3 {
                        acc[c] += to_lin[prev[i + c] as usize];
                    }
                    acc[3] += prev[i + 3] as f32 / 255.0;
                }
                let o = ((y * nw + x) * 4) as usize;
                for c in 0..3 {
                    next[o + c] = encode(acc[c] * 0.25);
                }
                next[o + 3] = (acc[3] * 0.25 * 255.0 + 0.5) as u8;
            }
        }
        out.push((next, (nw, nh)));
        pw = nw;
        ph = nh;
    }
    out
}

/// Block-compresses one level (padded to whole 4x4 blocks) on all cores.
fn compress(variant: CompressionVariant, rgba: &[u8], w: u32, h: u32, out: &mut Vec<u8>) {
    let (bw, bh) = (w.div_ceil(4) * 4, h.div_ceil(4) * 4);
    let padded;
    let src = if (bw, bh) == (w, h) {
        rgba
    } else {
        // Tiny mips (2x2, 1x1): repeat the edge.
        let mut p = vec![0u8; (bw * bh * 4) as usize];
        for y in 0..bh {
            for x in 0..bw {
                let s = ((y.min(h - 1) * w + x.min(w - 1)) * 4) as usize;
                let d = ((y * bw + x) * 4) as usize;
                p[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
            }
        }
        padded = p;
        &padded
    };
    let start = out.len();
    out.resize(start + variant.blocks_byte_size(bw, bh), 0);
    let dst = &mut out[start..];
    let rows = bh / 4;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8) as u32;
    let per = rows.div_ceil(threads).max(1);
    let row_bytes = variant.blocks_byte_size(bw, 4);
    std::thread::scope(|s| {
        for (i, chunk) in dst.chunks_mut(per as usize * row_bytes).enumerate() {
            let y0 = i as u32 * per * 4;
            let hh = (chunk.len() / row_bytes) as u32 * 4;
            let part = &src[(y0 * bw * 4) as usize..((y0 + hh) * bw * 4) as usize];
            s.spawn(move || compress_rgba8(variant, part, chunk, bw, hh, bw * 4));
        }
    });
}

fn cache_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) if cfg!(target_os = "macos") => PathBuf::from(home).join("Library/Caches/aquarium/textures"),
        _ => std::env::temp_dir().join("aquarium-textures"),
    }
}

/// One file per source and processing: its name hashes the path, size and
/// date of the source, the kind, the format and the version.
fn cache_path(file: &Path, kind: TexKind, bc: bool) -> Option<PathBuf> {
    let meta = std::fs::metadata(file).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (file, meta.len(), meta.modified().ok(), kind, bc, VERSION).hash(&mut hasher);
    let stem = file.file_stem()?.to_string_lossy();
    Some(cache_dir().join(format!("{stem}-{:016x}.aqtex", hasher.finish())))
}

const MAGIC: &[u8; 4] = b"AQTX";

fn read_cache(path: &Path, format: TextureFormat) -> Option<Image> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 16 || &bytes[0..4] != MAGIC {
        return None;
    }
    let word = |i: usize| u32::from_le_bytes(bytes[4 + 4 * i..8 + 4 * i].try_into().unwrap());
    let (w, h, levels) = (word(0), word(1), word(2));
    let data = bytes[16..].to_vec();
    // Sanity: the data must be exactly the chain the header announces.
    let expected: usize = (0..levels)
        .map(|l| {
            let (lw, lh) = ((w >> l).max(1), (h >> l).max(1));
            match format.block_dimensions() {
                (1, 1) => (lw * lh * 4) as usize,
                _ => (lw.div_ceil(4) * lh.div_ceil(4)) as usize * format.block_copy_size(None).unwrap_or(16) as usize,
            }
        })
        .sum();
    (data.len() == expected).then(|| gpu_image(UVec2::new(w, h), levels, format, data))
}

fn write_cache(path: &Path, size: UVec2, levels: u32, data: &[u8]) {
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        // Written aside then renamed: a crash never leaves half a file.
        let tmp = path.with_extension("tmp");
        let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        f.write_all(MAGIC)?;
        for v in [size.x, size.y, levels] {
            f.write_all(&v.to_le_bytes())?;
        }
        f.write_all(data)?;
        f.flush()?;
        drop(f);
        std::fs::rename(&tmp, path)
    };
    if let Err(e) = write() {
        warn!("texture cache {}: {e}", path.display());
    }
}

//! Measures BC encoding speed and quality on the real textures.
use block_compression::{BC7Settings, CompressionVariant, decode::decompress_blocks_as_rgba8, encode::compress_rgba8};
use std::time::Instant;

fn psnr(a: &[u8], b: &[u8], channels: &[usize]) -> f64 {
    let mut se = 0.0f64;
    let mut n = 0.0f64;
    for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        for &c in channels {
            let d = pa[c] as f64 - pb[c] as f64;
            se += d * d;
            n += 1.0;
        }
    }
    10.0 * (255.0f64 * 255.0 / (se / n)).log10()
}

fn compress_par(v: CompressionVariant, rgba: &[u8], w: u32, h: u32, threads: u32) -> Vec<u8> {
    let mut out = vec![0u8; v.blocks_byte_size(w, h)];
    let rows = h / 4;
    let per = rows.div_ceil(threads);
    let row_bytes = v.blocks_byte_size(w, 4);
    std::thread::scope(|s| {
        for (i, chunk) in out.chunks_mut((per as usize) * row_bytes).enumerate() {
            let y0 = i as u32 * per * 4;
            let hh = (chunk.len() / row_bytes) as u32 * 4;
            let src = &rgba[(y0 * w * 4) as usize..((y0 + hh) * w * 4) as usize];
            s.spawn(move || compress_rgba8(v, src, chunk, w, hh, w * 4));
        }
    });
    out
}

fn main() {
    let dir = "assets/models/rock_09/textures/rock_09";
    for (name, variant, ch) in [
        ("diff", CompressionVariant::BC7(BC7Settings::opaque_ultra_fast()), vec![0, 1, 2]),
        ("diff", CompressionVariant::BC7(BC7Settings::opaque_very_fast()), vec![0, 1, 2]),
        ("diff", CompressionVariant::BC7(BC7Settings::opaque_fast()), vec![0, 1, 2]),
        ("diff", CompressionVariant::BC1, vec![0, 1, 2]),
        ("arm", CompressionVariant::BC7(BC7Settings::opaque_ultra_fast()), vec![0, 1, 2]),
        ("arm", CompressionVariant::BC1, vec![0, 1, 2]),
        ("nor_gl", CompressionVariant::BC5, vec![0, 1]),
    ] {
        let t = Instant::now();
        let img = image::open(format!("{dir}_{name}_2k.jpg")).unwrap().to_rgba8();
        let (w, h) = img.dimensions();
        let dec = t.elapsed();
        let t = Instant::now();
        let blocks = compress_par(variant, &img, w, h, 6);
        let enc = t.elapsed();
        let mut back = vec![0u8; (w * h * 4) as usize];
        decompress_blocks_as_rgba8(variant, w, h, &blocks, &mut back);
        println!(
            "{name:7} {:?}: decode jpeg {:.0} ms, encode {:.0} ms (6 threads), {:.1} MB -> {:.1} MB, PSNR {:.2} dB",
            match variant { CompressionVariant::BC7(_) => "BC7".to_string(), v => format!("{v:?}") },
            dec.as_secs_f64() * 1e3,
            enc.as_secs_f64() * 1e3,
            img.len() as f64 / 1e6,
            blocks.len() as f64 / 1e6,
            psnr(&img, &back, &ch)
        );
    }
}

#define_import_path aquarium::fish_swim

// Per-species constants (material uniform, binding 100).
struct FishParams {
    // x: species id, y: fish length (m), z: tail amplitude scale, w: body wavelength (lengths)
    species: vec4<f32>,
    // Pattern colours (linear RGB), meaning depends on the species.
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    color_c: vec4<f32>,
    color_d: vec4<f32>,
    // x: fin opacity, y: iridescence, z: scale sparkle, w: body roughness
    look: vec4<f32>,
}

struct SwimState {
    phase: f32,
    amplitude: f32,
    omega: f32,
    turn: f32,
}

// The per-fish swim state is packed in the mesh tag (see src/fish/mod.rs):
// bits 0-11 phase, 12-17 amplitude, 18-23 angular frequency, 24-31 turn.
fn decode_swim(tag: u32) -> SwimState {
    var s: SwimState;
    s.phase = f32(tag & 0xfffu) / 4096.0 * 6.2831853;
    s.amplitude = f32((tag >> 12u) & 0x3fu) / 63.0;
    s.omega = f32((tag >> 18u) & 0x3fu) * 0.8;
    s.turn = f32((tag >> 24u) & 0xffu) / 127.5 - 1.0;
    return s;
}

// Lateral body wave travelling from head to tail. Returns the local-space
// displacement (x) and its slope along z (to bend the normals).
// `fin_t`: 0 at a fin root, 1 at its tip (pectoral fins only).
fn swim_wave(local: vec3<f32>, part: f32, fin_t: f32, s: SwimState, time_offset: f32, p: FishParams) -> vec2<f32> {
    let length = p.species.y;
    // 0 around the head pivot (a quarter of the way down the body), growing to the tail.
    let along = clamp((local.z + 0.2 * length) / length, 0.0, 1.3);
    let envelope = s.amplitude * p.species.z * length * (0.03 + 0.2 * along * along);
    let k = 6.2831853 / (p.species.w * length);
    let phase = s.phase + s.omega * time_offset;
    let wave = sin(k * local.z - phase);
    var dx = envelope * wave;
    var slope = envelope * k * cos(k * local.z - phase);
    // Body curls into turns.
    let bend = s.turn * 0.35 * along * along * length;
    dx += bend;
    slope += s.turn * 0.7 * along;
    // Pectoral fins paddle on their own (hovering fish keep them busy).
    if part > 2.5 {
        dx += sign(local.x) * fin_t * 0.035 * length * sin(phase * 1.3 + 1.0);
    }
    return vec2<f32>(dx, slope);
}

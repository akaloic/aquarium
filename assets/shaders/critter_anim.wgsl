#define_import_path aquarium::critter_anim

// Bottom dwellers (see src/benthos): one material per species, the per-instance
// state packed in the mesh tag, the body parts tagged in UV_1:
//   uv_b.x = part, uv_b.y = per-vertex parameter (phase offset / radius / 0..1 along).

struct CritterParams {
    // x: kind (0 crab, 3 flatfish, 4 starfish), y: size (m),
    // z: pattern scale, w: seed
    kind: vec4<f32>,
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    color_c: vec4<f32>,
    // x: translucency, y: water ambient (nits), z: roughness, w: wave amplitude
    look: vec4<f32>,
}

struct CritterState {
    // Locomotion phase (rad) and amplitude 0..1.
    phase: f32,
    amplitude: f32,
    // 0..1: how deep in the sand (buried), or how curled (starfish).
    buried: f32,
    // 0..1: alert (raised antennae / arms).
    alert: f32,
}

// Tag: bits 0-11 phase, 12-17 amplitude, 18-23 buried, 24-31 alert.
fn decode_critter(tag: u32) -> CritterState {
    var s: CritterState;
    s.phase = f32(tag & 0xfffu) / 4096.0 * 6.2831853;
    s.amplitude = f32((tag >> 12u) & 0x3fu) / 63.0;
    s.buried = f32((tag >> 18u) & 0x3fu) / 63.0;
    s.alert = f32((tag >> 24u) & 0xffu) / 255.0;
    return s;
}

// Local displacement of a vertex (the meshes are built around the origin,
// +Y up, head towards -Z like the fish). `param` = k + f: an integer phase
// index k and a fraction f (0..1 along the limb / radius).
fn critter_offset(p: vec3<f32>, part: f32, param: f32, s: CritterState, t: f32, c: CritterParams) -> vec3<f32> {
    let size = c.kind.y;
    let k = floor(param);
    let f = param - k;
    var d = vec3<f32>(0.0);
    if part > 4.5 && part < 5.5 {
        // Flatfish: vertical body wave, strong in the fin fringe (f 0..1).
        let kw = 6.2831853 / (0.8 * size);
        let along = clamp((p.z / size) + 0.5, 0.0, 1.0);
        let wave = sin(p.z * kw + s.phase);
        d.y += wave * (0.02 + 0.08 * along * along + 0.05 * f) * size * s.amplitude;
    } else if part > 5.5 && part < 6.5 {
        // Starfish arms: tips lift and slowly wave (f = radius 0..1).
        let r = f * f;
        let wave = sin(t * 0.35 + atan2(p.z, p.x) * 2.0 + c.kind.w);
        d.y += (0.06 + 0.04 * wave + 0.12 * s.alert) * size * r;
    }
    return d;
}

#define_import_path aquarium::ripples

#import aquarium::noise::value_noise2

// Gradient (dh/dx, dh/dz) of a small-scale water height field. A few capillary
// wave trains whose directions wander and whose wavefronts are broken up by
// phase noise, plus animated noise ripples. `strength` scales the slopes.
fn ripple_gradient(p: vec2<f32>, t: f32, strength: f32) -> vec2<f32> {
    var g = vec2<f32>(0.0);
    // (base angle, wavelength m, slope amplitude, speed m/s)
    let waves = array<vec4<f32>, 4>(
        vec4<f32>(0.35, 0.085, 0.060, 0.090),
        vec4<f32>(2.10, 0.052, 0.045, 0.075),
        vec4<f32>(-1.20, 0.033, 0.032, 0.060),
        vec4<f32>(2.90, 0.021, 0.022, 0.048),
    );
    for (var i = 0; i < 4; i++) {
        let w = waves[i];
        let fi = f32(i);
        let angle = w.x + 0.35 * sin(t * 0.07 + fi * 1.7);
        let d = vec2<f32>(cos(angle), sin(angle));
        let k = 6.2831853 / w.y;
        // Phase noise: irregular, non-repeating wavefronts.
        let phase_noise = value_noise2(p * (3.0 + fi * 1.3) + vec2<f32>(fi * 7.1, t * 0.05)) * 9.0;
        // Amplitude varies across the surface (wave groups).
        let group = 0.35 + 0.65 * value_noise2(p * 4.0 - d * t * 0.3 + vec2<f32>(fi * 3.3, 0.0));
        let arg = k * (dot(d, p) - w.w * t) + phase_noise;
        g += d * (w.z * group * cos(arg));
    }
    // Fine animated capillary noise (two octaves, central differences).
    let e = 0.0025;
    for (var o = 0; o < 2; o++) {
        let f = 30.0 * (1.0 + f32(o) * 1.9);
        let q = p * f + vec2<f32>(t * (0.5 + f32(o) * 0.3), -t * 0.35);
        let nx = value_noise2(q + vec2<f32>(e * f, 0.0)) - value_noise2(q - vec2<f32>(e * f, 0.0));
        let nz = value_noise2(q + vec2<f32>(0.0, e * f)) - value_noise2(q - vec2<f32>(0.0, e * f));
        g += vec2<f32>(nx, nz) / (2.0 * e) * (0.00022 / (1.0 + f32(o)));
    }
    return g * strength;
}

// Rings spreading from things dropped on the water. Each drop is
// (x, z, start time, strength). Returns (dh/dx, dh/dz, h).
fn drop_wave(p: vec2<f32>, t: f32, drop: vec4<f32>) -> vec3<f32> {
    let age = t - drop.z;
    if age < 0.0 || age > 4.5 || drop.w <= 0.0 {
        return vec3<f32>(0.0);
    }
    let d = p - drop.xy;
    let r = length(d);
    // Capillary-gravity waves: ~23 cm/s, the packet widens and the wavelength
    // grows as it travels, the amplitude drops with distance and time.
    let x = r - age * 0.23;
    let width = 0.012 + 0.035 * age;
    let env = exp(-x * x / (width * width)) * drop.w * exp(-age * 1.0) / (1.0 + r * 14.0);
    let k = 6.2831853 / (0.018 + 0.014 * age);
    let s = sin(k * x);
    let c = cos(k * x);
    let dh = env * (k * c - 2.0 * x / (width * width) * s);
    return vec3<f32>(d / max(r, 1e-4) * dh, env * s);
}

fn drops_wave(p: vec2<f32>, t: f32, drops: array<vec4<f32>, 4>) -> vec3<f32> {
    var w = vec3<f32>(0.0);
    for (var i = 0; i < 4; i++) {
        w += drop_wave(p, t, drops[i]);
    }
    return w;
}

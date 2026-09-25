#define_import_path aquarium::noise

// PCG-based integer hashes (Jarzynski & Olano, "Hash Functions for GPU Rendering", 2020).
fn pcg2d(v_in: vec2<u32>) -> vec2<u32> {
    var v = v_in * 1664525u + 1013904223u;
    v.x += v.y * 1664525u;
    v.y += v.x * 1664525u;
    v = v ^ (v >> vec2<u32>(16u));
    v.x += v.y * 1664525u;
    v.y += v.x * 1664525u;
    v = v ^ (v >> vec2<u32>(16u));
    return v;
}

fn pcg3d(v_in: vec3<u32>) -> vec3<u32> {
    var v = v_in * 1664525u + 1013904223u;
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v = v ^ (v >> vec3<u32>(16u));
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    return v;
}

// Hash of an integer lattice point (given as floats), in [0, 1).
fn hash22(p: vec2<f32>) -> vec2<f32> {
    let u = bitcast<vec2<u32>>(vec2<i32>(floor(p)));
    return vec2<f32>(pcg2d(u)) * (1.0 / 4294967296.0);
}

fn hash12(p: vec2<f32>) -> f32 {
    return hash22(p).x;
}

fn hash13(p: vec3<f32>) -> f32 {
    let u = bitcast<vec3<u32>>(vec3<i32>(floor(p)));
    return f32(pcg3d(u).x) * (1.0 / 4294967296.0);
}

fn value_noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash12(i);
    let b = hash12(i + vec2<f32>(1.0, 0.0));
    let c = hash12(i + vec2<f32>(0.0, 1.0));
    let d = hash12(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn value_noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let n000 = hash13(i);
    let n100 = hash13(i + vec3<f32>(1.0, 0.0, 0.0));
    let n010 = hash13(i + vec3<f32>(0.0, 1.0, 0.0));
    let n110 = hash13(i + vec3<f32>(1.0, 1.0, 0.0));
    let n001 = hash13(i + vec3<f32>(0.0, 0.0, 1.0));
    let n101 = hash13(i + vec3<f32>(1.0, 0.0, 1.0));
    let n011 = hash13(i + vec3<f32>(0.0, 1.0, 1.0));
    let n111 = hash13(i + vec3<f32>(1.0, 1.0, 1.0));
    let x00 = mix(n000, n100, u.x);
    let x10 = mix(n010, n110, u.x);
    let x01 = mix(n001, n101, u.x);
    let x11 = mix(n011, n111, u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

fn fbm2(p_in: vec2<f32>) -> f32 {
    var p = p_in;
    var a = 0.5;
    var s = 0.0;
    for (var i = 0; i < 4; i++) {
        s += a * value_noise2(p);
        p = mat2x2<f32>(1.6, 1.2, -1.2, 1.6) * p + vec2<f32>(1.7, 9.2);
        a *= 0.5;
    }
    return s / 0.9375;
}

fn fbm3(p_in: vec3<f32>) -> f32 {
    var p = p_in;
    var a = 0.5;
    var s = 0.0;
    for (var i = 0; i < 4; i++) {
        s += a * value_noise3(p);
        p = p * 2.03 + vec3<f32>(1.7, 9.2, 4.1);
        a *= 0.5;
    }
    return s / 0.9375;
}

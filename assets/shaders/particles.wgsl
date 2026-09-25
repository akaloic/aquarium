// GPU-animated particles: each quad carries a seed; the vertex shader moves it
// (drifting motes, rising bubbles) and billboards it. No CPU work per frame.

#import bevy_pbr::{
    mesh_functions,
    forward_io::Vertex,
    view_transformations::position_world_to_clip,
    mesh_view_bindings::{globals, view},
}
#import aquarium::caustics::shaft_pattern
#import aquarium::noise::value_noise3

struct ParticleParams {
    // rgb: tint, a: brightness (cd/m²-ish, scaled by exposure)
    color: vec4<f32>,
    // x: kind (0 motes, 1 bubbles), y: size (m), z: density 0..1, w: water surface y
    params: vec4<f32>,
    // Tank inner half extents (xyz), w: speed multiplier
    bounds: vec4<f32>,
    // Cursor "finger" in the water: start + strength, end + radius, motion dir.
    cursor_a: vec4<f32>,
    cursor_b: vec4<f32>,
    cursor_v: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> particles: ParticleParams;

struct VOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) light: f32,
    @location(2) fade: f32,
}

fn wrap(v: f32, lo: f32, hi: f32) -> f32 {
    return lo + fract((v - lo) / (hi - lo)) * (hi - lo);
}

@vertex
fn vertex(vertex: Vertex) -> VOut {
    var out: VOut;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let base = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0)).xyz;
    let corner = vertex.uv * 2.0 - 1.0;
    let seed = vertex.uv_b;
    let t = globals.time * particles.bounds.w;
    let kind = particles.params.x;
    let water_y = particles.params.w;
    let half = particles.bounds.xyz;

    var p: vec3<f32>;
    var size = particles.params.y * (0.55 + 0.9 * seed.x * seed.x);
    var fade = 1.0;

    if kind < 0.5 {
        // Suspended motes / plankton: slow drift with a gentle current plus
        // brownian-like wandering, wrapped inside the tank.
        let drift = vec3<f32>(0.006, 0.0015 * sin(seed.y * 40.0), 0.002);
        let wander = vec3<f32>(
            value_noise3(vec3<f32>(seed * 17.0, t * 0.05)) - 0.5,
            value_noise3(vec3<f32>(seed.yx * 23.0, t * 0.04 + 7.0)) - 0.5,
            value_noise3(vec3<f32>(seed * 31.0 + 3.0, t * 0.05 + 13.0)) - 0.5,
        ) * 0.06;
        p = base + drift * t + wander;
        p.x = wrap(p.x, -half.x, half.x);
        p.z = wrap(p.z, -half.z, half.z);
        p.y = wrap(p.y, 0.02, water_y - 0.01);
        // Fade near the wrap boundaries so nothing pops.
        let edge = min(min(half.x - abs(p.x), half.z - abs(p.z)), min(p.y - 0.02, water_y - 0.01 - p.y));
        fade = smoothstep(0.0, 0.03, edge);
        // Plankton twinkle as they tumble.
        fade *= 0.55 + 0.45 * sin(t * (1.0 + 3.0 * seed.y) + seed.x * 60.0);
        // The cursor stirs the water: specks near the finger are pushed aside,
        // dragged along its motion and swirl around it.
        let strength = particles.cursor_a.w;
        if strength > 0.001 {
            let a = particles.cursor_a.xyz;
            let ab = particles.cursor_b.xyz - a;
            let s = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-6), 0.0, 1.0);
            let off = p - (a + ab * s);
            let d = length(off);
            let radius = particles.cursor_b.w;
            let k = strength * exp(-d * d / (radius * radius));
            let radial = off / max(d, 1e-4);
            let axis = normalize(ab + vec3<f32>(0.0, 1e-5, 0.0));
            let swirl = cross(axis, radial) * (0.6 + 0.8 * seed.y);
            p += (radial * 0.9 + swirl * 1.3) * (k * radius * 0.6)
                + particles.cursor_v.xyz * (k * radius * 0.8);
        }
    } else {
        // Micro-bubbles rising from an emitter at `base`.
        let rise_speed = 0.06 + 0.07 * seed.y;
        let height = water_y - base.y;
        let phase = fract(t * rise_speed / height + seed.x);
        let y = base.y + phase * height;
        let climb = phase * height;
        let wobble = vec2<f32>(
            sin(t * (7.0 + 5.0 * seed.x) + seed.y * 30.0),
            cos(t * (6.0 + 4.0 * seed.y) + seed.x * 20.0),
        ) * (0.0015 + 0.004 * phase);
        let spread = vec2<f32>(seed.x - 0.5, seed.y - 0.5) * (0.01 + 0.05 * climb);
        p = vec3<f32>(base.x + wobble.x + spread.x, y, base.z + wobble.y + spread.y);
        // Bubbles grow slightly as the pressure drops.
        size *= 0.8 + 0.4 * phase;
        fade = smoothstep(0.0, 0.03, phase) * (1.0 - smoothstep(0.93, 1.0, phase));
    }

    // Density control: collapse particles beyond the current budget.
    if fract(seed.x * 7.13 + seed.y * 3.71) > particles.params.z {
        size = 0.0;
    }

    // Light from the surface pattern (same field that shapes the god rays).
    // The luminaire position mirrors the main spot light in environment.rs.
    let to_light = normalize(vec3<f32>(0.08, 2.05, 0.20) - p);
    let surface_p = p + to_light * ((water_y - p.y) / max(to_light.y, 0.2));
    let shaft = shaft_pattern(surface_p.xz, globals.time);
    let depth = water_y - p.y;
    out.light = (0.18 + 1.6 * shaft) * exp(-depth * 1.2);

    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    let world = p + (right * corner.x + up * corner.y) * size;
    out.position = position_world_to_clip(world);
    out.uv = corner;
    out.fade = fade;
    return out;
}

@fragment
fn fragment(in: VOut) -> @location(0) vec4<f32> {
    let r = length(in.uv);
    if r > 1.0 {
        discard;
    }
    var c: vec3<f32>;
    var a: f32;
    if particles.params.x < 0.5 {
        // Soft glowing speck.
        a = pow(1.0 - r, 2.2);
        c = particles.color.rgb;
    } else {
        // Bubble: dark-ish core, bright refracting rim and a specular dot.
        let rim = smoothstep(0.55, 0.92, r) * (1.0 - smoothstep(0.92, 1.0, r));
        let spec = 1.0 - smoothstep(0.0, 0.28, length(in.uv - vec2<f32>(-0.35, 0.4)));
        a = rim * 0.9 + spec * 1.4 + 0.08;
        c = particles.color.rgb;
    }
    let brightness = particles.color.a * in.light * in.fade * view.exposure;
    // Additive (premultiplied with zero alpha): colour carries the energy.
    return vec4<f32>(c * a * brightness, 0.0);
}

#define_import_path aquarium::sway

struct SwayParams {
    // Displacement (metres) at full flex.
    amplitude: f32,
    // Temporal frequency multiplier.
    frequency: f32,
    // Vertices are kept under this height (floating leaves stay in the water).
    water_y: f32,
    // 0 = plants (current-driven), 1 = tentacles (writhing).
    mode: f32,
}

// Displacement of a vertex at rest position `rest`, `flex` in 0..1 (0 = rooted),
// `phase` a per-strand random value. Spatially coherent so neighbouring blades
// move together like in a real current.
fn sway_offset(rest: vec3<f32>, flex: f32, phase: f32, t: f32, p: SwayParams) -> vec3<f32> {
    let w = t * p.frequency;
    var off: vec3<f32>;
    if p.mode < 0.5 {
        let k = rest.x * 3.1 + rest.z * 1.7;
        let gust = 0.65 + 0.35 * sin(w * 0.31 + rest.x * 1.3 + rest.z);
        let dx = (sin(w + k + phase * 0.8) * 0.65 + sin(w * 1.93 + k * 1.7 + phase * 2.3) * 0.25) * gust + 0.3;
        let dz = sin(w * 0.83 + k * 1.2 + phase * 1.3 + 1.7) * 0.55 + sin(w * 2.3 + phase * 3.1 + rest.y * 9.0) * 0.18;
        // Travelling ripple along the strand.
        let ripple = sin(w * 2.7 - rest.y * 18.0 + phase * 6.0) * 0.12;
        off = vec3<f32>(dx + ripple, 0.0, dz + ripple * 0.6) * (p.amplitude * flex);
    } else {
        // Tentacles: faster, more chaotic, mostly perpendicular waving.
        let a = w * 1.7 + phase * 6.2831853;
        let dx = sin(a + rest.y * 40.0) * 0.6 + sin(a * 1.9 + 1.3) * 0.4;
        let dz = cos(a * 1.3 + rest.y * 33.0) * 0.6 + sin(a * 2.3 + 0.7) * 0.4;
        let dy = sin(a * 0.7) * 0.25;
        off = vec3<f32>(dx, dy, dz) * (p.amplitude * flex);
    }
    // Bending (roughly) preserves length: displaced tips come down a little.
    let bend = dot(off.xz, off.xz);
    off.y -= bend * 3.0 * flex;
    return off;
}

// Inner half extents of the tank minus a small margin (see tank.rs).
const TANK_HALF: vec2<f32> = vec2<f32>(0.688, 0.288);

fn apply_sway(world: vec3<f32>, flex: f32, phase: f32, t: f32, p: SwayParams) -> vec3<f32> {
    var w = world + sway_offset(world, flex, phase, t, p);
    w.y = min(w.y, p.water_y);
    w.x = clamp(w.x, -TANK_HALF.x, TANK_HALF.x);
    w.z = clamp(w.z, -TANK_HALF.y, TANK_HALF.y);
    return w;
}

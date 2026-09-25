#define_import_path aquarium::caustics

#import aquarium::noise::value_noise2

// Bright interconnected caustic network. `p_world` in metres. Roughly 0..1.6.
// Ridges (iso-lines) of two domain-warped, animated noise layers: a cheap
// approximation of the focusing network produced by surface ripples.
fn caustic_pattern(p_world: vec2<f32>, t: f32, scale: f32) -> f32 {
    let p = p_world * scale;
    let warp = vec2<f32>(
        value_noise2(p * 0.45 + vec2<f32>(t * 0.06, 0.0)),
        value_noise2(p * 0.45 + vec2<f32>(5.2, -t * 0.05)),
    ) - 0.5;
    let q = p + warp * 1.6;
    let n1 = value_noise2(q + vec2<f32>(t * 0.23, t * 0.11));
    let n2 = value_noise2(q * 1.73 + vec2<f32>(3.1 - t * 0.17, t * 0.21));
    let r1 = 1.0 - abs(n1 * 2.0 - 1.0);
    let r2 = 1.0 - abs(n2 * 2.0 - 1.0);
    let l1 = r1 * r1 * r1 * r1;
    let l2 = r2 * r2 * r2 * r2;
    return l1 * l1 * 0.9 + l2 * l2 * 0.55 + l1 * l2 * 0.7;
}

// Large, slowly drifting bright/dark patches: long surface waves focusing the light.
// These become the broad god-ray shafts in the volumetric water. Built from a few
// domain-warped sines only: it is evaluated at every ray-march step of the water
// volume, so it must stay cheap.
fn shaft_pattern(p_world: vec2<f32>, t: f32) -> f32 {
    // Organic cells a few centimetres across (the surface ripples focusing the
    // light), drifting slowly, modulated by a broad variation over the tank.
    let p = p_world;
    let warp = vec2<f32>(sin(p.y * 17.0 + t * 0.35), sin(p.x * 15.0 - t * 0.29 + 1.9)) * 0.028;
    let q = p + warp;
    let fine = sin(q.x * 41.0 + sin(q.y * 23.0 + t * 0.31) * 1.4 + t * 0.23)
        * sin(q.y * 36.0 - sin(q.x * 19.0 - t * 0.27) * 1.3 - t * 0.19);
    let broad = sin(q.x * 7.3 + q.y * 4.1 + t * 0.06) * sin(q.y * 6.1 - q.x * 2.3 - t * 0.05 + 0.7);
    let v = 0.5 + 0.38 * fine + 0.2 * broad;
    return smoothstep(0.52, 0.9, v);
}

// Fraction of the light that reaches the water at this surface point.
fn surface_transmission(p_world: vec2<f32>, t: f32, base: f32, caustic_gain: f32, shaft_gain: f32, scale: f32) -> f32 {
    // Surfaces see the shafts softened (light scattered on the way down).
    let shaft = mix(0.4, 1.0, shaft_pattern(p_world, t));
    let c = caustic_pattern(p_world, t * 1.1, scale);
    let lit = shaft * shaft_gain + c * caustic_gain * (0.45 + 0.55 * shaft);
    return clamp(base + (1.0 - base) * lit, 0.0, 1.0);
}

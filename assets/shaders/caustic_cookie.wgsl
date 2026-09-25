// Animated caustics "cookie" projected by the main spot light.
//
// Rendered every frame into a texture by a fullscreen pass (caustics.rs). Each texel
// corresponds to a direction inside the spot cone (Bevy's spot light texture
// mapping); we intersect that ray with the water surface and evaluate how much
// light the rippling surface focuses there.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import aquarium::caustics::surface_transmission
#import aquarium::ripples::drops_wave

struct CookieParams {
    // xyz: light position, w: tan(outer angle)
    light_pos: vec4<f32>,
    // light local +X in world space, w: water surface height
    right: vec4<f32>,
    // light local +Y in world space, w: light fraction always transmitted
    up: vec4<f32>,
    // light local +Z in world space (the light looks down -Z), w: caustic gain
    back: vec4<f32>,
    // x: shaft gain, y: caustic cells per metre, z: time scale, w: time (s)
    params: vec4<f32>,
    // Rings from dropped food (x, z, start time, strength).
    drops: array<vec4<f32>, 4>,
}

@group(0) @binding(0) var<uniform> cookie: CookieParams;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let time = cookie.params.w;
    let tan_a = cookie.light_pos.w;
    // Inverse of Bevy's `decal_uv = (xy / (z * tan)) * (-0.5, 0.5) + 0.5` with z = -1.
    let local = vec3<f32>((2.0 * in.uv.x - 1.0) * tan_a, (1.0 - 2.0 * in.uv.y) * tan_a, -1.0);
    let dir = normalize(cookie.right.xyz * local.x + cookie.up.xyz * local.y + cookie.back.xyz * local.z);
    let water_y = cookie.right.w;
    let t = (water_y - cookie.light_pos.y) / min(dir.y, -1e-3);
    let p = cookie.light_pos.xyz + dir * t;
    let v = surface_transmission(
        p.xz, time * cookie.params.z, cookie.up.w, cookie.back.w, cookie.params.x, cookie.params.y
    );
    // The rings focus and defocus the light: bright and dark caustic circles
    // racing across the floor (curvature ~ -k² h).
    let ring = drops_wave(p.xz, time, cookie.drops).z;
    let lit = v * clamp(1.0 - ring * 1400.0, 0.25, 2.2);
    return vec4<f32>(lit, lit, lit, 1.0);
}

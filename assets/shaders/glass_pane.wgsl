// Flat glass panes: Fresnel reflection of the room, blended over the scene.
// Much cheaper than full-screen screen-space transmission, and identical for a
// thin flat pane (which barely deviates rays).

#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}

struct PaneParams {
    // x: F0, y: environment intensity, z: tint opacity
    params: vec4<f32>,
    // Tint seen through the glass edge-on (greenish), linear RGB.
    tint: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> pane: PaneParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var env_texture: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var env_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let v = normalize(view.world_position - in.world_position.xyz);
    var n = normalize(in.world_normal);
    if dot(n, v) < 0.0 {
        n = -n;
    }
    let cos_t = saturate(dot(n, v));
    let f = pane.params.x + (1.0 - pane.params.x) * pow(1.0 - cos_t, 5.0);
    let env = textureSampleLevel(env_texture, env_sampler, reflect(-v, n), 0.0).rgb;
    // Longer path through the glass at grazing angles: a touch more tint.
    let tint = pane.params.z * (1.0 + 2.0 * pow(1.0 - cos_t, 3.0));
    let reflected = env * pane.params.y * view.exposure * f;
    // Premultiplied: the scene behind is attenuated by (1 - alpha).
    return vec4(reflected + pane.tint.rgb * tint * view.exposure, f + tint);
}

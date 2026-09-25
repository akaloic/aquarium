// Water surface seen from above: a transmissive StandardMaterial (IOR 1.33)
// whose normal is perturbed by animated micro-ripples.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{globals, view},
}
#import aquarium::ripples::{ripple_gradient, drops_wave}

struct SurfaceParams {
    // x: ripple strength, y: time scale, z: camera-distance boost, w: unused
    ripple: vec4<f32>,
    // Rings from dropped food (x, z, start time, strength).
    drops: array<vec4<f32>, 4>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> surface: SurfaceParams;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Ripples are more pronounced when the camera gets close to the surface.
    let cam_dist = distance(view.world_position, in.world_position.xyz);
    let near = 1.0 + surface.ripple.z * (1.0 - smoothstep(0.15, 1.2, cam_dist));
    let g = ripple_gradient(in.world_position.xz, globals.time * surface.ripple.y, surface.ripple.x * near)
        + drops_wave(in.world_position.xz, globals.time, surface.drops).xy;
    let n = normalize(vec3<f32>(-g.x, 1.0, -g.y));
    pbr_input.N = n;
    pbr_input.clearcoat_N = n;

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}

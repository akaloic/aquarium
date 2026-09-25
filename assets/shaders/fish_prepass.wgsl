// Prepass / shadow shader for fish: same swim deformation (and the previous
// frame's, for motion vectors) and the same stochastic fin discard as fish.wgsl.

#import bevy_pbr::{
    mesh_functions,
    prepass_io::{Vertex, VertexOutput},
    prepass_bindings,
    view_transformations::position_world_to_clip,
    mesh_view_bindings::view,
    utils::interleaved_gradient_noise,
}
#import bevy_render::globals::Globals
#import aquarium::fish_swim::{FishParams, decode_swim, swim_wave}

// Prepass view bind group: view (0), globals (1), previous view (2).
@group(0) @binding(1) var<uniform> prepass_globals: Globals;
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> fish: FishParams;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let swim = decode_swim(mesh_functions::get_tag(vertex.instance_index));
    let part = vertex.uv_b.x;
    let fin_t = select(0.0, vertex.uv.x, part > 2.5);
    let w = swim_wave(vertex.position, part, fin_t, swim, 0.0, fish);
    let local = vertex.position + vec3<f32>(w.x, 0.0, 0.0);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(local, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);

#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif

    out.uv = vertex.uv;
    out.uv_b = vertex.uv_b;

#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
    let n = normalize(vec3<f32>(vertex.normal.x, vertex.normal.y, vertex.normal.z - w.y * vertex.normal.x));
    out.world_normal = mesh_functions::mesh_normal_local_to_world(n, vertex.instance_index);
#endif

#ifdef MOTION_VECTOR_PREPASS
    let prev_from_local = mesh_functions::get_previous_world_from_local(vertex.instance_index);
    let wp = swim_wave(vertex.position, part, fin_t, swim, -prepass_globals.delta_time, fish);
    let prev_local = vertex.position + vec3<f32>(wp.x, 0.0, 0.0);
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(prev_from_local, vec4<f32>(prev_local, 1.0));
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

// Mirrors the fin opacity of fish.wgsl.
fn fin_alpha(in: VertexOutput) -> f32 {
    let part = in.uv_b.x;
    if part < 0.5 || (part > 1.5 && part < 2.5) {
        return 1.0;
    }
    let species = u32(fish.species.x + 0.5);
    let u = in.uv.x;
    let v = in.uv.y;
    var alpha = fish.look.x * (0.75 + 0.25 * sin(v * 90.0)) * (1.0 - 0.35 * u);
    if species == 1u {
        alpha = max(alpha, 0.55 * smoothstep(0.55, 0.9, sin(v * 16.0)));
    } else if species == 2u {
        alpha = 0.92;
    } else if species == 3u {
        alpha = max(alpha, 0.8);
    }
    return clamp(alpha, 0.0, 1.0);
}

fn fin_discard(in: VertexOutput) {
    let alpha = fin_alpha(in);
    if alpha < 0.999 && interleaved_gradient_noise(in.position.xy, prepass_globals.frame_count) > alpha {
        discard;
    }
}

#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    fin_discard(in);
    var out: FragmentOutput;
#ifdef NORMAL_PREPASS
    out.normal = vec4(in.world_normal * 0.5 + vec3(0.5), 1.0);
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
#ifdef MOTION_VECTOR_PREPASS
    let clip_position_t = view.unjittered_clip_from_world * in.world_position;
    let clip_position = clip_position_t.xy / clip_position_t.w;
    let previous_clip_position_t = prepass_bindings::previous_view_uniforms.clip_from_world * in.previous_world_position;
    let previous_clip_position = previous_clip_position_t.xy / previous_clip_position_t.w;
    out.motion_vector = (clip_position - previous_clip_position) * vec2(0.5, -0.5);
#endif
    return out;
}
#else
@fragment
fn fragment(in: VertexOutput) {
    fin_discard(in);
}
#endif

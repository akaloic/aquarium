// Prepass / shadow vertex shader matching plant.wgsl (depth, motion vectors
// with the previous frame's sway, and shadow maps).

#import bevy_pbr::{
    mesh_functions,
    prepass_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}
#import bevy_render::globals::Globals
#import aquarium::sway::{SwayParams, apply_sway}

// Prepass view bind group: view (0), globals (1), previous view (2).
@group(0) @binding(1) var<uniform> prepass_globals: Globals;
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> sway: SwayParams;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let rest = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));

#ifdef VERTEX_UVS_B
    let flex = vertex.uv_b.x;
    let phase = vertex.uv_b.y;
#else
    let flex = 0.0;
    let phase = 0.0;
#endif

    let t = prepass_globals.time;
    let world = apply_sway(rest.xyz, flex, phase, t, sway);
    out.world_position = vec4<f32>(world, 1.0);
    out.position = position_world_to_clip(world);

#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif

#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local, vertex.tangent, vertex.instance_index
    );
#endif
#endif

#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif

#ifdef MOTION_VECTOR_PREPASS
    let prev_from_local = mesh_functions::get_previous_world_from_local(vertex.instance_index);
    let prev_rest = mesh_functions::mesh_position_local_to_world(prev_from_local, vec4<f32>(vertex.position, 1.0));
    let prev = apply_sway(prev_rest.xyz, flex, phase, t - prepass_globals.delta_time, sway);
    out.previous_world_position = vec4<f32>(prev, 1.0);
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

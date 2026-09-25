// Prepass / shadow pass for the bottom dwellers: same limb animation as
// critter.wgsl, previous frame for the motion vectors.

#import bevy_pbr::{
    mesh_functions,
    prepass_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}
#import bevy_render::globals::Globals
#import aquarium::critter_anim::{CritterParams, decode_critter, critter_offset}

@group(0) @binding(1) var<uniform> prepass_globals: Globals;
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> critter: CritterParams;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let s = decode_critter(mesh_functions::get_tag(vertex.instance_index));
    let t = prepass_globals.time;
    let local = vertex.position + critter_offset(vertex.position, vertex.uv_b.x, vertex.uv_b.y, s, t, critter);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(local, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
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
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
#ifdef MOTION_VECTOR_PREPASS
    let prev_from_local = mesh_functions::get_previous_world_from_local(vertex.instance_index);
    let prev_local = vertex.position + critter_offset(vertex.position, vertex.uv_b.x, vertex.uv_b.y, s, t - prepass_globals.delta_time, critter);
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(prev_from_local, vec4<f32>(prev_local, 1.0));
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

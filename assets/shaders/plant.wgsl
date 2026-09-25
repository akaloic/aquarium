// Main-pass vertex shader for plants / anemones: StandardMaterial shading with
// procedural swaying. UV_1 carries (flex, phase) per vertex.

#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::globals,
}
#import aquarium::sway::{SwayParams, apply_sway}

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

    let world = apply_sway(rest.xyz, flex, phase, globals.time, sway);
    out.world_position = vec4<f32>(world, 1.0);
    out.position = position_world_to_clip(world);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local, vertex.tangent, vertex.instance_index
    );
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

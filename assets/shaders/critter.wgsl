// Bottom dwellers: StandardMaterial lighting with procedural limb animation
// (critter_anim.wgsl) and species patterns.

#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::globals,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#import aquarium::critter_anim::{CritterParams, decode_critter, critter_offset}
#import aquarium::noise::{value_noise2, hash22}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> critter: CritterParams;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let s = decode_critter(mesh_functions::get_tag(vertex.instance_index));
    let local = vertex.position + critter_offset(vertex.position, vertex.uv_b.x, vertex.uv_b.y, s, globals.time, critter);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(local, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
    out.uv = vertex.uv;
    out.uv_b = vertex.uv_b;
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

// Ocelli: orange rings with a dark centre on a jittered grid.
fn ocelli(uv: vec2<f32>, scale: f32) -> vec2<f32> {
    let g = uv * scale;
    let cell = floor(g);
    var ring = 0.0;
    var core = 0.0;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let c = cell + vec2<f32>(f32(i), f32(j));
            let h = hash22(c * 1.37 + 5.1);
            let centre = c + 0.2 + 0.6 * h;
            let r = 0.18 + 0.12 * h.y;
            let d = length(g - centre);
            ring = max(ring, smoothstep(r + 0.05, r, d) * smoothstep(r * 0.45, r * 0.62, d));
            core = max(core, smoothstep(r * 0.55, r * 0.4, d));
        }
    }
    return vec2<f32>(ring, core);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let s = decode_critter(mesh_functions::get_tag(in.instance_index));
    let kind = u32(critter.kind.x + 0.5);
    let part = in.uv_b.x;
    var color = pbr_input.material.base_color.rgb;
    var rough = critter.look.z;
    // Facing up (dorsal) or down (belly).
    let up = in.world_normal.y;

    if part > 6.5 && part < 7.5 {
        // Eyes: black, glossy.
        color = vec3<f32>(0.01);
        rough = 0.05;
    } else if kind == 0u {
        // Crab: fine speckles on the shell, wet sheen.
        let n = value_noise2(in.uv * critter.kind.z) * 0.6 + value_noise2(in.uv * critter.kind.z * 3.1) * 0.4;
        color *= 0.75 + 0.5 * smoothstep(0.3, 0.8, n);
        color = mix(color, critter.color_b.rgb, smoothstep(0.72, 0.8, n) * 0.6);
    } else if kind == 3u {
        // Flatfish: sand camouflage, a few orange spots, white blind side.
        let m = value_noise2(in.uv * critter.kind.z) * 0.55 + value_noise2(in.uv * critter.kind.z * 2.7 + 3.0) * 0.45;
        var top = mix(critter.color_a.rgb * 0.7, critter.color_a.rgb * 1.15, m);
        let spots = ocelli(in.uv, critter.kind.z * 0.5);
        top = mix(top, critter.color_b.rgb, spots.y * 0.7);
        color = mix(vec3<f32>(0.85, 0.83, 0.8), top, smoothstep(-0.2, 0.2, up));
    } else if kind == 4u {
        // Starfish: knobbly orange top, pale underside with tube-feet grooves.
        let knobs = ocelli(in.uv * vec2<f32>(5.0, 1.0), critter.kind.z);
        var top = critter.color_a.rgb * (0.8 + 0.3 * value_noise2(in.uv * 30.0));
        top = mix(top, critter.color_b.rgb, knobs.y);
        let groove = smoothstep(0.02, 0.0, abs(fract(in.uv.x * 5.0) - 0.5) - 0.04);
        let under = mix(critter.color_c.rgb, critter.color_c.rgb * 0.6, groove);
        color = mix(under, top, smoothstep(-0.3, 0.3, up));
        rough = mix(0.8, rough, knobs.y);
    }

    // Buried: a dusting of sand over the back.
    if s.buried > 0.0 {
        let grains = value_noise2(in.world_position.xz * 420.0) * 0.5 + value_noise2(in.world_position.xz * 90.0) * 0.5;
        let dust = smoothstep(0.45, 0.8, grains) * s.buried * 0.55;
        color = mix(color, vec3<f32>(0.55, 0.46, 0.34), dust * smoothstep(-0.1, 0.4, up));
    }

    pbr_input.material.base_color = vec4<f32>(color, 1.0);
    pbr_input.material.perceptual_roughness = rough;
    // Light scattered by the water all around (brighter from above): without
    // it, faces turned away from the lamp would be black.
    let n = normalize(in.world_normal);
    pbr_input.material.emissive = vec4<f32>(color * critter.look.y * (0.45 + 0.55 * (n.y * 0.5 + 0.5)), 1.0);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}

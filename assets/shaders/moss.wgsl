// Scanned rocks / driftwood with a procedural moss layer growing on the
// upward-facing, sheltered parts, plus a "wet stone" look.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
}
#import aquarium::noise::{fbm3, value_noise3}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::forward_io::{VertexOutput, FragmentOutput}
#endif

struct MossParams {
    // rgb: moss colour (linear), a: unused
    color: vec4<f32>,
    // x: coverage (0..1), y: noise scale (1/m), z: seed, w: wetness (0..1)
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> moss: MossParams;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    let wp = in.world_position.xyz;
    let geo_n = normalize(in.world_normal);
    let coverage = moss.params.x;
    let scale = moss.params.y;
    let seed = moss.params.z;
    let wet = moss.params.w;

    // Wet rock: darker albedo, glossier.
    let base = pbr_input.material.base_color.rgb * mix(1.0, 0.78, wet);
    let rough = pbr_input.material.perceptual_roughness * mix(1.0, 0.72, wet);

    // Moss mask: favours upward faces and crevices (ambient occlusion), broken
    // up by two octaves of noise.
    let ao = pbr_input.diffuse_occlusion.x;
    let broad = fbm3(wp * scale + vec3<f32>(seed));
    let fine = value_noise3(wp * scale * 11.0 + vec3<f32>(seed * 3.1));
    let up = clamp(geo_n.y, -1.0, 1.0);
    var m = up * 0.9 + (broad - 0.5) * 1.6 + (1.0 - ao) * 0.35 - (1.0 - coverage) * 1.4;
    m = smoothstep(0.05, 0.55, m);
    // Fuzzy, tufted edge.
    m *= smoothstep(0.15, 0.55, fine * 0.6 + m * 0.7);

    let tint = mix(vec3<f32>(0.75, 0.85, 0.55), vec3<f32>(1.15, 1.1, 0.8), fine);
    let moss_col = moss.color.rgb * tint * (0.65 + 0.7 * broad);

    pbr_input.material.base_color = vec4<f32>(mix(base, moss_col, m), pbr_input.material.base_color.a);
    pbr_input.material.perceptual_roughness = mix(rough, 0.92, m);
    pbr_input.material.reflectance = mix(pbr_input.material.reflectance, vec3<f32>(0.25), m);
    // Moss hides the rock's micro relief and scatters light through its fronds.
    pbr_input.N = normalize(mix(pbr_input.N, geo_n, m * 0.6));
    pbr_input.material.diffuse_transmission = m * 0.25;

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif
    return out;
}

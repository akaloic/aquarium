// Fish: StandardMaterial lighting (caustics, shadows...) with procedural swim
// deformation, species colour patterns, iridescence, translucent fins and
// glossy eyes.

#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::{globals, view},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    utils::interleaved_gradient_noise,
}
#import aquarium::fish_swim::{FishParams, decode_swim, swim_wave}
#import aquarium::noise::value_noise2

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
    // Bend the normal with the body wave.
    let n = normalize(vec3<f32>(vertex.normal.x, vertex.normal.y, vertex.normal.z - w.y * vertex.normal.x));

    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(local, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(n, vertex.instance_index);
    out.uv = vertex.uv;
    out.uv_b = vertex.uv_b;
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

struct Look {
    color: vec3<f32>,
    roughness: f32,
    metallic: f32,
    reflectance: f32,
    emissive: vec3<f32>,
    alpha: f32,
}

fn band(x: f32, a: f32, b: f32, soft: f32) -> f32 {
    return smoothstep(a - soft, a + soft, x) * (1.0 - smoothstep(b - soft, b + soft, x));
}

// Body colouring. u: along (0 nose, 1 tail base), v: height (0 belly, 1 back).
fn body_look(u: f32, v: f32, n_dot_v: f32) -> Look {
    var l: Look;
    l.roughness = fish.look.w;
    l.metallic = 0.0;
    l.reflectance = 0.5;
    l.emissive = vec3<f32>(0.0);
    l.alpha = 1.0;
    let species = u32(fish.species.x + 0.5);
    // Counter-shading: lighter belly on every species.
    let belly = smoothstep(0.35, 0.05, v);

    if species == 0u {
        // Neon tetra: dark olive back, iridescent blue line, red rear belly.
        var c = mix(fish.color_c.rgb, fish.color_d.rgb, smoothstep(0.52, 0.86, v));
        let red = smoothstep(0.40, 0.47, u) * (1.0 - smoothstep(0.48, 0.54, v)) * (1.0 - smoothstep(0.9, 1.0, u)) * smoothstep(0.1, 0.22, v);
        c = mix(c, fish.color_b.rgb, red);
        let stripe = band(v, 0.53, 0.7, 0.015) * band(u, 0.1, 0.88, 0.03);
        // Structural colour: shifts from cyan to deep blue with the view angle.
        let irid = mix(fish.color_a.rgb, vec3<f32>(0.05, 0.12, 0.85), pow(1.0 - n_dot_v, 1.5));
        c = mix(c, irid, stripe);
        l.color = c;
        // The structural colours catch any light: keep them luminous.
        l.emissive = irid * stripe * fish.look.y + fish.color_b.rgb * red * fish.look.y * 0.25;
        l.roughness = mix(l.roughness, 0.18, stripe);
        l.reflectance = mix(0.5, 0.9, stripe);
    } else if species == 1u {
        // Angelfish: silver with dark vertical bars, golden forehead.
        var c = mix(fish.color_a.rgb, fish.color_c.rgb, smoothstep(0.75, 1.0, v) * smoothstep(0.35, 0.1, u));
        let bu = u + (v - 0.5) * 0.08;
        let bars = max(max(band(bu, 0.07, 0.13, 0.012), band(bu, 0.36, 0.45, 0.015)), band(bu, 0.68, 0.75, 0.015));
        c = mix(c, fish.color_b.rgb, bars * 0.92);
        l.color = c;
        l.metallic = 0.35 * (1.0 - bars);
        l.roughness = 0.28;
        l.emissive = fish.color_d.rgb * pow(1.0 - n_dot_v, 3.0) * fish.look.y * (1.0 - bars);
    } else if species == 2u {
        // Clownfish: orange with three white, black-edged bands.
        let bu = u - 0.05 * (v - 0.5) * (v - 0.5);
        let w1 = band(bu, 0.16, 0.26, 0.006);
        let w2 = band(bu, 0.47, 0.57, 0.006);
        let w3 = band(bu, 0.89, 0.97, 0.006);
        let white = max(max(w1, w2), w3);
        let e1 = band(bu, 0.145, 0.275, 0.004);
        let e2 = band(bu, 0.455, 0.585, 0.004);
        let e3 = band(bu, 0.875, 0.985, 0.004);
        let edge = max(max(e1, e2), e3) * (1.0 - white);
        var c = mix(fish.color_a.rgb, fish.color_b.rgb, white);
        c = mix(c, fish.color_c.rgb, edge);
        l.color = c;
        l.roughness = 0.3;
    } else {
        // Discus: warm base with wavy turquoise lines and faint vertical bars.
        let wav = sin((v * 15.0 + sin(u * 11.0) * 1.3 + value_noise2(vec2<f32>(u, v) * 9.0) * 1.2) * 3.14159);
        let lines = smoothstep(0.45, 0.8, wav) * smoothstep(0.02, 0.15, u) * smoothstep(0.02, 0.12, v) * (1.0 - smoothstep(0.9, 0.98, v));
        let bars = 0.28 * smoothstep(0.6, 1.0, sin(u * 3.14159 * 9.0));
        var c = mix(fish.color_a.rgb, fish.color_b.rgb, lines);
        c *= 1.0 - bars;
        l.color = c;
        l.emissive = fish.color_b.rgb * lines * pow(1.0 - n_dot_v, 2.0) * fish.look.y;
        l.roughness = 0.32;
    }
    l.color = mix(l.color, max(l.color, vec3<f32>(0.55, 0.57, 0.6)), belly * 0.6);
    return l;
}

// Fins: u from root (0) to edge (1), v along the fin.
fn fin_look(u: f32, v: f32) -> Look {
    var l: Look;
    l.roughness = 0.45;
    l.metallic = 0.0;
    l.reflectance = 0.35;
    l.emissive = vec3<f32>(0.0);
    let species = u32(fish.species.x + 0.5);
    // Fin rays.
    let rays = 0.75 + 0.25 * sin(v * 90.0);
    var alpha = fish.look.x * rays * (1.0 - 0.35 * u);
    var c = fish.color_d.rgb;
    if species == 1u {
        // Angelfish: bars run through the fins, dark trailing streaks.
        let streak = smoothstep(0.55, 0.9, sin(v * 16.0));
        c = mix(c, fish.color_b.rgb, streak * 0.7);
        alpha = max(alpha, 0.55 * streak);
    } else if species == 2u {
        // Clownfish: orange fins with a black margin.
        let margin = smoothstep(0.78, 0.9, u);
        c = mix(fish.color_a.rgb, fish.color_c.rgb, margin);
        alpha = 0.92;
    } else if species == 3u {
        // Discus: blue edge.
        c = mix(fish.color_a.rgb, fish.color_b.rgb, smoothstep(0.55, 0.95, u));
        alpha = max(alpha, 0.8);
    }
    l.color = c;
    l.alpha = clamp(alpha, 0.0, 1.0);
    return l;
}

fn eye_look(u: f32) -> Look {
    var l: Look;
    // u: 1 at the pupil centre.
    let pupil = smoothstep(0.93, 0.95, u);
    let iris = smoothstep(0.78, 0.8, u) * (1.0 - pupil);
    l.color = mix(vec3<f32>(0.02), fish.color_d.rgb * 1.2, iris);
    l.color = mix(l.color, vec3<f32>(0.004), pupil);
    l.roughness = 0.06;
    l.metallic = 0.0;
    l.reflectance = 1.0;
    l.emissive = vec3<f32>(0.0);
    l.alpha = 1.0;
    return l;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let part = in.uv_b.x;
    let n_dot_v = clamp(abs(dot(pbr_input.N, pbr_input.V)), 0.0, 1.0);

    var l: Look;
    if part < 0.5 {
        l = body_look(in.uv.x, in.uv.y, n_dot_v);
    } else if part < 1.5 || part > 2.5 {
        l = fin_look(in.uv.x, in.uv.y);
    } else {
        l = eye_look(in.uv.x);
    }

    // Translucent fins: stochastic transparency, resolved by TAA. The prepass
    // shader discards exactly the same pixels.
    if l.alpha < 0.999 {
        let noise = interleaved_gradient_noise(in.position.xy, globals.frame_count);
        if noise > l.alpha {
            discard;
        }
        pbr_input.material.diffuse_transmission = 0.5;
    }

    // Tiny scale sparkle on the body.
    if part < 0.5 {
        let scales = value_noise2(vec2<f32>(in.uv.x * 90.0, in.uv.y * 45.0));
        l.color *= 1.0 + fish.look.z * (scales - 0.5);
    }

    pbr_input.material.base_color = vec4<f32>(l.color, 1.0);
    pbr_input.material.perceptual_roughness = l.roughness;
    pbr_input.material.metallic = l.metallic;
    pbr_input.material.reflectance = vec3<f32>(l.reflectance);
    pbr_input.material.emissive = vec4<f32>(l.emissive, 1.0);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}

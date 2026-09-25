// Underside of the water surface, seen from below (through the front glass or
// when diving). Outside Snell's window (~48.6° from vertical) the surface is a
// shimmering total-internal-reflection mirror; inside it we see the dim room
// and the bright luminaire, refracted by the ripples.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{globals, view},
}
#import aquarium::ripples::{ripple_gradient, drops_wave}
#import aquarium::noise::value_noise2

struct UndersideParams {
    // Colour (cd/m²) of the distant water reflected by total internal reflection.
    mirror_color: vec4<f32>,
    // Colour of the lit sand bed, seen in the mirror when looking steeply.
    floor_color: vec4<f32>,
    // Colour of the room seen through Snell's window.
    sky_color: vec4<f32>,
    // Luminaire position (xyz) and brightness (w).
    light: vec4<f32>,
    // x: ripple strength, y: time scale
    ripple: vec4<f32>,
    // Rings from dropped food (x, z, start time, strength).
    drops: array<vec4<f32>, 4>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: UndersideParams;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xyz;
    let t = globals.time * params.ripple.y;
    let g = ripple_gradient(p.xz, t, params.ripple.x) + drops_wave(p.xz, globals.time, params.drops).xy;
    // Normal pointing down into the water.
    let n = normalize(vec3<f32>(g.x, -1.0, g.y));
    let v = normalize(view.world_position - p);
    let cos_i = clamp(dot(v, n), 0.0, 1.0);

    // Ray leaving the water towards the air (eta = n_water / n_air).
    let r = refract(-v, n, 1.33);
    let tir = dot(r, r) < 1e-4;

    // Mirror: the reflected ray looks back down into the tank. Modulate the
    // reflected brightness with the ripples (it alternately catches the lit
    // sand and the dark background).
    let refl = reflect(-v, n);
    // The reflected ray looks down into the tank: towards the sand when steep,
    // towards the far (foggy) water when grazing.
    let steep = smoothstep(0.15, 0.75, -refl.y);
    let sparkle = value_noise2(p.xz * 26.0 + refl.xz * 9.0 + vec2<f32>(t * 0.35, 0.0));
    let streak = value_noise2(vec2<f32>(p.x * 7.0 + refl.x * 4.0, p.z * 60.0) + vec2<f32>(0.0, t * 0.2));
    let caught = 0.6 + 0.8 * smoothstep(0.35, 0.95, sparkle) * streak + 4.0 * length(g);
    var color = mix(params.mirror_color.rgb, params.floor_color.rgb, steep) * caught;

    if !tir {
        // Schlick Fresnel for water -> air.
        let f0 = 0.02;
        let cos_t = clamp(dot(r, -n), 0.0, 1.0);
        let fres = f0 + (1.0 - f0) * pow(1.0 - cos_t, 5.0);
        let l = normalize(params.light.xyz - p);
        let glint = pow(max(dot(normalize(r), l), 0.0), 2500.0) * params.light.w;
        let halo = pow(max(dot(normalize(r), l), 0.0), 40.0) * params.light.w * 0.0004;
        let window = params.sky_color.rgb + vec3<f32>(glint + halo);
        // Soft edge of Snell's window.
        let edge = smoothstep(0.0, 0.25, cos_t);
        color = mix(color, window * (1.0 - fres) + color * fres, edge);
    }

    return vec4<f32>(color * view.exposure, 1.0);
}

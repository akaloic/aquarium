// Replacement for Bevy's `volumetric_fog.wgsl`, tuned for a single water volume
// lit by one volumetric spot light (the aquarium luminaire).
//
// Same bindings / uniform layout as the original, so Bevy's volumetric fog
// render pass drives it unchanged. Differences:
//   * the volumetric light is located once per pixel instead of walking the
//     cluster light list at every ray-march step (much cheaper),
//   * the number of steps adapts to the length of water crossed,
//   * no directional-light / atmosphere paths,
//   * `ambient_color * ambient_intensity` is used as a density-proportional
//     in-scatter term (multiple scattering), so shadowed water is never black,
//   * light shafts: the light reaching each sample is modulated by the same
//     surface pattern that drives the caustics, evaluated where the light ray
//     crosses the water surface (`scattering_asymmetry` carries the surface
//     height, since there is no directional light / phase function here).

#import bevy_pbr::mesh_view_bindings::{globals, lights, view, clustered_lights}
#import bevy_pbr::mesh_view_types::{
    POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    POINT_LIGHT_FLAGS_VOLUMETRIC_BIT,
    POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE,
}
#import bevy_pbr::shadow_sampling::{sample_shadow_map, sample_shadow_cubemap, SPOT_SHADOW_TEXEL_SIZE}
#import bevy_pbr::utils::interleaved_gradient_noise
#import bevy_pbr::view_transformations::{
    depth_ndc_to_view_z,
    frag_coord_to_ndc,
    position_ndc_to_view,
    position_ndc_to_world,
    position_view_to_world,
}
#import bevy_render::maths::orthonormalize
#import bevy_pbr::clustered_forward as clustering
#import bevy_pbr::lighting::getDistanceAttenuation;
#import aquarium::caustics::shaft_pattern
#import aquarium::noise::hash22

// Light fraction outside the shafts, and the average once they have blurred out.
const SHAFT_BASE: f32 = 0.12;
const SHAFT_MEAN: f32 = 0.3;
// Light left in the plants' and rocks' shade.
const SHADE_FLOOR: f32 = 0.45;
// Target ray-march step length in metres.
const STEP_LENGTH: f32 = 0.1;

struct VolumetricFog {
    clip_from_local: mat4x4<f32>,
    uvw_from_world: mat4x4<f32>,
    far_planes: array<vec4<f32>, 6>,
    fog_color: vec3<f32>,
    light_tint: vec3<f32>,
    ambient_color: vec3<f32>,
    ambient_intensity: f32,
    step_count: u32,
    bounding_radius: f32,
    absorption: f32,
    scattering: f32,
    density_factor: f32,
    density_texture_offset: vec3<f32>,
    scattering_asymmetry: f32,
    light_intensity: f32,
    jitter_strength: f32,
}

@group(1) @binding(0) var<uniform> volumetric_fog: VolumetricFog;

#ifdef MULTISAMPLED
@group(1) @binding(1) var depth_texture: texture_depth_multisampled_2d;
#else
@group(1) @binding(1) var depth_texture: texture_depth_2d;
#endif

#ifdef DENSITY_TEXTURE
@group(1) @binding(2) var density_texture: texture_3d<f32>;
@group(1) @binding(3) var density_sampler: sampler;
#endif

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
}

@vertex
fn vertex(vertex: Vertex) -> @builtin(position) vec4<f32> {
    return volumetric_fog.clip_from_local * vec4<f32>(vertex.position, 1.0);
}

// Spot light shadow at `p_world`, the light's frame and constants computed
// once per pixel by the caller (they don't change along the ray).
fn spot_shadow(p_world: vec3<f32>, light_pos: vec3<f32>, light_inv_rot: mat3x3<f32>, depth_bias: f32,
               tan_angle: f32, shadow_index: i32, frag_coord_xy: vec2<f32>) -> f32 {
    let surface_to_light = light_pos - p_world;
    let offset_position = -surface_to_light + depth_bias * normalize(surface_to_light);
    let projected_position = offset_position * light_inv_rot;
    let f_div_minus_z = 1.0 / (tan_angle * -projected_position.z);
    let shadow_xy_ndc = projected_position.xy * f_div_minus_z;
    let shadow_uv = shadow_xy_ndc * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    // Bevy spot shadow maps use a fixed 0.1 near plane.
    let depth = 0.1 / -projected_position.z;
    return sample_shadow_map(shadow_uv, depth, shadow_index, frag_coord_xy, SPOT_SHADOW_TEXEL_SIZE);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let step_count = volumetric_fog.step_count;
    let absorption = volumetric_fog.absorption;
    let scattering = volumetric_fog.scattering;
    let density_factor = volumetric_fog.density_factor;
    let exposure = view.exposure;
    let frag_coord = position;

    // End of the ray: scene depth or the far side of the volume.
    let ndc_end_depth_from_buffer = textureLoad(depth_texture, vec2<i32>(frag_coord.xy), 0);
    let view_end_depth_from_buffer = -position_ndc_to_view(
        frag_coord_to_ndc(vec4(position.xy, ndc_end_depth_from_buffer, 1.0))).z;
    let view_start_pos = position_ndc_to_view(frag_coord_to_ndc(frag_coord));

    // Far side of the volume: ray-box slabs in its [0, 1]³ texture space, the
    // ray going from the camera (t = 0) through the front face (t = 1). Same
    // exit as testing the six face planes against each other, without the
    // 36 dot products and branches per pixel.
    let cam_uvw = (volumetric_fog.uvw_from_world * vec4(view.world_position, 1.0)).xyz;
    let ray_uvw = (volumetric_fog.uvw_from_world * (view.world_from_view * vec4(view_start_pos.xyz, 0.0))).xyz;
    let inv_ray = 1.0 / ray_uvw;
    let t_far = max(-cam_uvw * inv_ray, (vec3(1.0) - cam_uvw) * inv_ray);
    let t_exit = max(0.0, min(min(t_far.x, t_far.y), t_far.z));
    var end_depth_view = -view_start_pos.z * t_exit;
    end_depth_view = min(end_depth_view, view_end_depth_from_buffer);
    let start_depth_view = -depth_ndc_to_view_z(frag_coord.z);
    let ray_length_view = max(0.0, end_depth_view - start_depth_view);
    if (ray_length_view == 0.0) {
        return vec4(0.0);
    }

    // Step count follows the length of water crossed (the uniform is the cap),
    // TAA + jitter do the rest.
    let steps = u32(clamp(ceil(ray_length_view / STEP_LENGTH), 4.0, f32(step_count)));
    let step_size_world = ray_length_view / f32(steps);
    let Rd_ndc = vec3(frag_coord_to_ndc(position).xy, 1.0);
    let Rd_view = normalize(position_ndc_to_view(Rd_ndc));
    var Ro_world = position_view_to_world(view_start_pos.xyz);
    let Rd_world = normalize(position_ndc_to_world(Rd_ndc) - view.world_position);
    // Jitter by a fraction of a step (`jitter_strength`): TAA integrates the
    // offsets over frames, so few long steps look like many short ones.
    let jitter = interleaved_gradient_noise(position.xy, globals.frame_count) * volumetric_fog.jitter_strength * step_size_world;
    Ro_world += Rd_world * jitter;

#ifdef DENSITY_TEXTURE
    let uvw_from_world = volumetric_fog.uvw_from_world;
    let Ro_uvw = (uvw_from_world * vec4(Ro_world, 1.0)).xyz;
    let Rd_step_uvw = mat3x3(uvw_from_world[0].xyz, uvw_from_world[1].xyz, uvw_from_world[2].xyz) *
        (Rd_world * step_size_world);
#endif

    // Locate the volumetric light once, from the cluster at the ray start.
    let is_orthographic = view.clip_from_view[3].w == 1.0;
    let cluster_index = clustering::view_fragment_cluster_index(frag_coord.xy, view_start_pos.z, is_orthographic);
    let ranges = clustering::unpack_clusterable_object_index_ranges(cluster_index);
    var light_id = 0xffffffffu;
    var is_spot = false;
    for (var i: u32 = ranges.first_point_light_index_offset; i < ranges.first_reflection_probe_index_offset; i = i + 1u) {
        let id = clustering::get_clusterable_object_id(i);
        if ((clustered_lights.data[id].flags & POINT_LIGHT_FLAGS_VOLUMETRIC_BIT) != 0u) {
            light_id = id;
            is_spot = i >= ranges.first_spot_light_index_offset;
            break;
        }
    }

    // Per-light constants.
    var light_pos = vec3(0.0);
    var light_color = vec3(0.0);
    var inv_range_sq = 0.0;
    var spot_dir = vec3(0.0, -1.0, 0.0);
    var spot_scale = 0.0;
    var spot_offset = 1.0;
    var shadows = false;
    var depth_bias = 0.0;
    var tan_angle = 1.0;
    if (light_id != 0xffffffffu) {
        let light = &clustered_lights.data[light_id];
        depth_bias = (*light).shadow_depth_bias;
        tan_angle = (*light).spot_light_tan_angle;
        light_pos = (*light).position_radius.xyz;
        light_color = (*light).color_inverse_square_range.rgb;
        inv_range_sq = (*light).color_inverse_square_range.w;
        shadows = ((*light).flags & POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u;
        if (is_spot) {
            spot_dir = vec3<f32>((*light).light_custom_data.x, 0.0, (*light).light_custom_data.y);
            spot_dir.y = sqrt(max(0.0, 1.0 - spot_dir.x * spot_dir.x - spot_dir.z * spot_dir.z));
            if (((*light).flags & POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE) != 0u) {
                spot_dir.y = -spot_dir.y;
            }
            spot_scale = (*light).light_custom_data.z;
            spot_offset = (*light).light_custom_data.w;
        }
    }

    let extinction = absorption + scattering;
    let light_attenuation = exp(-density_factor * volumetric_fog.bounding_radius * extinction);
    let light_factor = volumetric_fog.fog_color * volumetric_fog.light_tint * light_attenuation *
        scattering * volumetric_fog.light_intensity * exposure * light_color;
    let ambient = volumetric_fog.ambient_color * volumetric_fog.ambient_intensity * exposure;
    let water_y = volumetric_fog.scattering_asymmetry;
    // The spot's frame and the plane the shadow samples are spread in.
    let light_inv_rot = orthonormalize(-spot_dir);
    let side_a = normalize(cross(spot_dir, vec3(0.0, 0.0, 1.0)));
    let side_b = cross(spot_dir, side_a);
    let shadow_index = i32(light_id) + lights.spot_light_shadowmap_offset;

    var accumulated_color = vec3(0.0);
    var transmittance = 1.0;
    for (var step = 0u; step < steps; step += 1u) {
        if (transmittance < 0.002) {
            break;
        }
        let P_world = Ro_world + Rd_world * f32(step) * step_size_world;

        var density = density_factor;
#ifdef DENSITY_TEXTURE
        let P_uvw = Ro_uvw + Rd_step_uvw * f32(step);
        if (all(P_uvw >= vec3(0.0)) && all(P_uvw <= vec3(1.0))) {
            density *= textureSampleLevel(density_texture, density_sampler, P_uvw + volumetric_fog.density_texture_offset, 0.0).r;
        } else {
            density = 0.0;
        }
#endif
        let sample_transmittance = exp(-step_size_world * density * extinction);

        var in_light = 0.0;
        if (light_id != 0xffffffffu) {
            let light_to_frag = light_pos - P_world;
            let distance_square = dot(light_to_frag, light_to_frag);
            in_light = getDistanceAttenuation(distance_square, inv_range_sq);
            if (is_spot) {
                let cd = dot(-spot_dir, light_to_frag * inverseSqrt(distance_square));
                let a = saturate(cd * spot_scale + spot_offset);
                in_light *= a * a;
                if (shadows && in_light > 0.0) {
                    // Soft volumetric shadows: the lookup is jittered sideways by a
                    // few centimetres (TAA resolves the noise), and some light always
                    // reaches the shade (multiple scattering in the water).
                    let j = hash22(position.xy + vec2(f32(step) * 7.31, f32(globals.frame_count % 64u) * 3.17)) * 2.0 - 1.0;
                    let q = P_world + (side_a * j.x + side_b * j.y) * 0.055;
                    in_light *= SHADE_FLOOR + (1.0 - SHADE_FLOOR) *
                        spot_shadow(q, light_pos, light_inv_rot, depth_bias, tan_angle, shadow_index, position.xy);
                }
                if (in_light > 0.0) {
                    let to_l = light_pos - P_world;
                    let s = P_world + to_l * ((water_y - P_world.y) / max(to_l.y, 1e-3));
                    // Shafts are sharpest just under the surface and blur out with depth.
                    let shaft = SHAFT_BASE + (1.0 - SHAFT_BASE) * shaft_pattern(s.xz, globals.time);
                    let sharp = exp(-(water_y - P_world.y) * 1.6);
                    in_light *= mix(SHAFT_MEAN, shaft, sharp);
                }
            }
        }

        // Energy-conserving integration of the in-scattered light over the step.
        let scatter = (light_factor * in_light + ambient) * density;
        accumulated_color += scatter * transmittance * (1.0 - sample_transmittance) / max(density * extinction, 1e-4);
        transmittance *= sample_transmittance;
    }

    return vec4(accumulated_color, 1.0 - transmittance);
}

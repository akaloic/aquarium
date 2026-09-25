//! Hardscape: photogrammetry rocks, driftwood and a shell (Poly Haven, CC0),
//! all shaded with the procedural moss material.

use std::collections::HashMap;

use bevy::{image::CompressedImageFormatSupport, prelude::*};

use super::{
    MossExt, MossParams, RockMaterial, sand_height,
    scans::{ScanLibrary, ScanPart},
};
use crate::{
    sdf::Decor,
    textures::{TexKind, TextureLibrary},
};

struct Piece {
    model: &'static str,
    res: &'static str,
    mesh: usize,
    /// Mesh-local centre to recentre multi-object scans.
    center: Vec3,
    scale: f32,
    /// World (x, z).
    at: Vec2,
    /// How deep the piece is pushed into the sand (m).
    sink: f32,
    /// Yaw, pitch, roll (radians).
    rot: Vec3,
    moss: f32,
}

const fn piece(model: &'static str, res: &'static str, mesh: usize) -> Piece {
    Piece {
        model,
        res,
        mesh,
        center: Vec3::ZERO,
        scale: 1.0,
        at: Vec2::ZERO,
        sink: 0.0,
        rot: Vec3::ZERO,
        moss: 0.4,
    }
}

fn layout() -> Vec<Piece> {
    let stones_centers = [
        vec3(-0.9035, 0.0, 0.002),
        vec3(-0.6715, 0.0, 0.0065),
        vec3(-0.419, 0.0, -0.008),
        vec3(-0.206, 0.0, -0.0015),
        vec3(0.0245, 0.0, -0.001),
    ];
    let mut v = vec![
        // Main stone (oyaishi), left of centre, leaning slightly.
        Piece {
            scale: 0.125,
            at: vec2(-0.24, -0.04),
            sink: 0.03,
            rot: vec3(1.25, -0.18, 0.12),
            moss: 0.55,
            ..piece("namaqualand_boulder_03", "2k", 0)
        },
        // Secondary group on the right.
        Piece {
            scale: 0.12,
            at: vec2(0.34, -0.077),
            sink: 0.022,
            rot: vec3(-0.35, 0.0, 0.0),
            moss: 0.5,
            ..piece("namaqualand_boulder_02", "2k", 0)
        },
        // Flat stone front-left.
        Piece {
            scale: 0.18,
            at: vec2(-0.50, 0.08),
            sink: 0.018,
            rot: vec3(0.75, 0.0, 0.05),
            moss: 0.45,
            ..piece("namaqualand_boulder_05", "2k", 0)
        },
        // Dark rugged accent stones.
        Piece {
            scale: 1.7,
            at: vec2(-0.03, 0.04),
            sink: 0.008,
            rot: vec3(0.4, 0.0, 0.0),
            moss: 0.35,
            ..piece("rock_09", "2k", 0)
        },
        Piece {
            scale: 1.25,
            at: vec2(0.56, 0.10),
            sink: 0.006,
            rot: vec3(-1.1, 0.0, 0.0),
            moss: 0.3,
            ..piece("rock_09", "2k", 0)
        },
        // Driftwood branch rising behind the right group.
        Piece {
            scale: 0.95,
            at: vec2(0.16, -0.17),
            sink: 0.02,
            rot: vec3(2.4, -0.28, 0.42),
            moss: 0.28,
            ..piece("dead_quiver_branch_01", "2k", 0)
        },
        // Shell on the sand path.
        Piece {
            scale: 0.62,
            at: vec2(0.14, 0.19),
            sink: 0.004,
            rot: vec3(0.9, 0.0, 0.08),
            moss: 0.0,
            ..piece("lambis_shell", "1k", 0)
        },
    ];
    // Satellite rocks around the main stones (placed clear of them: `AQ_OVERLAPS=1`
    // reports how deep any piece sinks into another).
    let satellites = [
        (0, vec2(-0.062, -0.122), 0.42, 0.3),
        (1, vec2(-0.408, -0.158), 0.45, 1.7),
        (2, vec2(0.17, -0.02), 0.40, 0.9),
        (3, vec2(0.50, -0.17), 0.42, 2.6),
        (1, vec2(-0.30, 0.13), 0.34, 4.0),
    ];
    for (mesh, at, scale, yaw) in satellites {
        v.push(Piece {
            scale,
            at,
            sink: 0.006,
            rot: vec3(yaw, 0.0, 0.0),
            moss: 0.35,
            ..piece("namaqualand_rocks_01", "1k", mesh)
        });
    }
    // Pebbles scattered in the foreground.
    let pebbles = [
        (0, vec2(-0.16, 0.20), 1.0),
        (1, vec2(0.05, 0.23), 0.8),
        (2, vec2(0.38, 0.18), 0.8),
        (3, vec2(-0.40, 0.22), 0.9),
        (4, vec2(0.26, 0.08), 0.7),
        (2, vec2(-0.62, 0.18), 0.8),
        (3, vec2(0.62, 0.22), 0.9),
        (0, vec2(-0.08, 0.12), 0.9),
    ];
    for (mesh, at, scale) in pebbles {
        v.push(Piece {
            center: stones_centers[mesh],
            scale,
            at,
            sink: 0.004,
            rot: vec3(at.x * 13.0, 0.0, 0.0),
            moss: 0.1,
            ..piece("namaqualand_stones_01", "1k", mesh)
        });
    }
    v
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_hardscape(
    mut commands: Commands,
    images: Res<Assets<Image>>,
    meshes: Res<Assets<Mesh>>,
    support: Option<Res<CompressedImageFormatSupport>>,
    mut textures: ResMut<TextureLibrary>,
    mut scans: ResMut<ScanLibrary>,
    mut materials: ResMut<Assets<RockMaterial>>,
) {
    let pieces = layout();
    // Each scan is simplified for the largest size it is shown at.
    let mut largest: HashMap<(&str, usize), f32> = HashMap::new();
    for p in &pieces {
        let s = largest.entry((p.model, p.mesh)).or_default();
        *s = s.max(p.scale);
    }
    let mut cache: HashMap<(&str, u32), Handle<RockMaterial>> = HashMap::new();
    for (i, p) in pieces.into_iter().enumerate() {
        let dir = format!("models/{0}/textures/{0}", p.model);
        let moss_key = (p.model, (p.moss * 100.0) as u32);
        let material = cache
            .entry(moss_key)
            .or_insert_with(|| {
                let mut tex = |name: &str, kind| textures.load(&images, support.as_deref(), &format!("{dir}_{name}_{}.jpg", p.res), kind);
                let arm = tex("arm", TexKind::Data);
                let diff = tex("diff", TexKind::Color);
                let nor = tex("nor_gl", TexKind::Normal);
                let wood = p.model.contains("branch");
                materials.add(RockMaterial {
                    base: StandardMaterial {
                        base_color_texture: Some(diff),
                        normal_map_texture: Some(nor),
                        metallic_roughness_texture: Some(arm.clone()),
                        occlusion_texture: Some(arm),
                        metallic: 0.0,
                        perceptual_roughness: 1.0,
                        reflectance: 0.4,
                        ..default()
                    },
                    extension: MossExt {
                        moss: MossParams {
                            color: if wood {
                                Vec4::new(0.045, 0.11, 0.018, 1.0)
                            } else {
                                Vec4::new(0.05, 0.13, 0.02, 1.0)
                            },
                            params: Vec4::new(p.moss, 7.0, i as f32 * 3.7, 0.8),
                        },
                    },
                })
            })
            .clone();

        let part = ScanPart {
            model: p.model,
            mesh: p.mesh,
        };
        let mesh = scans.load(&meshes, part.clone(), largest[&(p.model, p.mesh)]);
        let y = sand_height(p.at.x, p.at.y) - p.sink;
        let rotation = Quat::from_euler(EulerRot::YXZ, p.rot.x, p.rot.y, p.rot.z);
        commands
            .spawn((
                Name::new(p.model),
                Transform {
                    translation: vec3(p.at.x, y, p.at.y),
                    rotation,
                    scale: Vec3::splat(p.scale),
                },
                Visibility::default(),
            ))
            .with_children(|parent| {
                // Every piece goes into the decor's distance field.
                parent.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::from_translation(-p.center),
                    Decor,
                    part,
                ));
            });
    }
}

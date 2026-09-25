//! Air bubbles: occasional puffs from the fishes' mouths, splashes of food
//! breaking the surface, sand puffs... All drawn from a pre-allocated pool (no
//! entity is spawned or despawned while running).

use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};
use rand::{RngExt, rng};

use super::Fish;
use crate::tank::WATER_Y;

const POOL: usize = 96;

#[derive(Component, Default)]
pub struct Bubble {
    active: bool,
    vy: f32,
    phase: f32,
    radius: f32,
}

/// Bubbles requested by other systems this frame: (position, radius).
#[derive(Resource, Default)]
pub struct BubbleRequests(pub Vec<(Vec3, f32)>);

#[derive(Resource, Default)]
pub struct BubblePool {
    free: Vec<Entity>,
}

impl Bubble {
    /// Rising speed if in use (physics test).
    pub fn probe(&self) -> Option<f32> {
        self.active.then_some(self.vy)
    }
}

pub fn spawn_pool(
    mut commands: Commands,
    mut pool: ResMut<BubblePool>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap());
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.9, 0.97, 1.0, 0.22),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.04,
        reflectance: 1.0,
        ..default()
    });
    pool.free.reserve(POOL);
    for i in 0..POOL {
        // The first one stays visible (buried under the sand) for a few seconds
        // so its pipeline is compiled before the first real bubble appears.
        let warm = i == 0;
        let e = commands
            .spawn((
                Name::new("bubble"),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(0.0, if warm { -0.03 } else { -1.0 }, 0.1)
                    .with_scale(Vec3::splat(0.002)),
                if warm { Visibility::Visible } else { Visibility::Hidden },
                Bubble::default(),
                NotShadowCaster,
                NotShadowReceiver,
            ))
            .id();
        pool.free.push(e);
    }
}

/// Fish burp a few bubbles every now and then; requests from elsewhere are
/// served from the pool.
pub fn emit(
    time: Res<Time>,
    mut requests: ResMut<BubbleRequests>,
    mut pool: ResMut<BubblePool>,
    mut fish: Query<(&mut Fish, &Transform), Without<Bubble>>,
    mut bubbles: Query<(&mut Bubble, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    let mut rng = rng();
    for (mut f, tr) in &mut fish {
        f.bubble_timer -= dt;
        if f.bubble_timer > 0.0 {
            continue;
        }
        f.bubble_timer = rng.random_range(12.0..45.0);
        let mouth = tr.translation + tr.forward() * (0.45 * f.length);
        for k in 0..rng.random_range(1..4) {
            let radius = rng.random_range(0.0009..0.0017) * (f.length / 0.05).clamp(0.6, 1.6);
            requests.0.push((mouth + Vec3::Y * (k as f32 * 0.004), radius));
        }
    }
    for (pos, radius) in requests.0.drain(..) {
        let Some(e) = pool.free.pop() else {
            break;
        };
        if let Ok((mut b, mut tr, mut vis)) = bubbles.get_mut(e) {
            *b = Bubble {
                active: true,
                vy: 0.01,
                phase: rng.random_range(0.0..std::f32::consts::TAU),
                radius,
            };
            *tr = Transform::from_translation(pos).with_scale(Vec3::splat(radius));
            *vis = Visibility::Visible;
        }
    }
}

pub fn rise(
    time: Res<Time>,
    mut pool: ResMut<BubblePool>,
    mut bubbles: Query<(Entity, &mut Bubble, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs().min(0.05);
    let t = time.elapsed_secs();
    for (e, mut b, mut tr, mut vis) in &mut bubbles {
        if !b.active {
            // The warm-up bubble goes back to the pool once pipelines are ready.
            if t > 5.0 && *vis == Visibility::Visible {
                *vis = Visibility::Hidden;
            }
            continue;
        }
        // Terminal velocity grows with the bubble size.
        let terminal = 0.08 + b.radius * 45.0;
        b.vy += (terminal - b.vy) * (1.0 - (-dt * 3.0).exp());
        let wobble = Vec3::new((t * 9.0 + b.phase).sin(), 0.0, (t * 7.0 + b.phase).cos()) * 0.004;
        tr.translation += (Vec3::Y * b.vy + wobble) * dt;
        // Slightly larger as the pressure drops.
        tr.scale = Vec3::splat(b.radius * (1.0 + 0.3 * (tr.translation.y / WATER_Y)));
        if tr.translation.y > WATER_Y - 0.002 {
            b.active = false;
            *vis = Visibility::Hidden;
            pool.free.push(e);
        }
    }
}

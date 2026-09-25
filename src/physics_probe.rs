//! Physics test bench (`AQ_PHYSICS=dir`, headless with `--screenshot`):
//! a fixed 1/60 s time step (`AQ_PHYSICS_DT=30` for 1/30 s, `vsync` for the
//! irregular steps of a window missing refreshes), the autopilot cursor and
//! clicks, a forced startle every 20 s, and a CSV per population written every
//! frame — fish, the crab and each of its feet, flatfish and starfish, food
//! flakes, bubbles. The checks live in `scripts/physics_report.py`.

use std::{
    fs::File,
    io::{BufWriter, Write},
    sync::Mutex,
};

use bevy::{prelude::*, time::TimeUpdateStrategy};

use crate::{
    benthos::{Crab, FORCE_STARTLE, Glider, Star},
    fish::{Bubble, Fish},
    interaction::Flake,
    sdf::DecorSdf,
};

#[derive(Resource)]
struct Out {
    fish: Mutex<BufWriter<File>>,
    crab: Mutex<BufWriter<File>>,
    bottom: Mutex<BufWriter<File>>,
    flakes: Mutex<BufWriter<File>>,
    bubbles: Mutex<BufWriter<File>>,
    events: Mutex<BufWriter<File>>,
    frame: u32,
    /// Next forced startle (virtual seconds).
    next_startle: f32,
    dt_mode: String,
}

pub struct PhysicsProbePlugin;

impl Plugin for PhysicsProbePlugin {
    fn build(&self, app: &mut App) {
        let Ok(dir) = std::env::var("AQ_PHYSICS") else {
            return;
        };
        std::fs::create_dir_all(&dir).expect("physics output dir");
        let open = |name: &str, header: &str| {
            let mut f = BufWriter::new(File::create(format!("{dir}/{name}.csv")).expect("csv"));
            writeln!(f, "{header}").unwrap();
            Mutex::new(f)
        };
        let mut legs = String::new();
        for i in 0..8 {
            legs += &format!(",f{i}_sdf,f{i}_planted,f{i}_stretch,f{i}_x,f{i}_y,f{i}_z");
        }
        app.insert_resource(Out {
            fish: open("fish", "frame,t,id,species,len,px,py,pz,fx,fy,fz,ux,uy,uz,speed,sdf,panic,satiety"),
            crab: open("crab", &format!("frame,t,state,cx,cy,cz,ux,uy,uz,fx,fy,fz,speed,contact_sdf,centre_sdf,shell_rock,knee_rock,claw_rock{legs}")),
            bottom: open("bottom", "frame,t,kind,id,mode,px,py,pz,ux,uy,uz,speed,contact_sdf,buried,above_sand"),
            flakes: open("flakes", "frame,t,id,state,px,py,pz,vx,vy,vz,sdf"),
            bubbles: open("bubbles", "frame,t,id,py,vy"),
            events: open("events", "frame,t,event"),
            frame: 0,
            next_startle: 10.0,
            dt_mode: std::env::var("AQ_PHYSICS_DT").unwrap_or_default(),
        })
        .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)))
        .add_systems(Last, record);
    }
}

#[allow(clippy::too_many_arguments)]
fn record(
    mut out: ResMut<Out>,
    time: Res<Time>,
    mut step: ResMut<TimeUpdateStrategy>,
    sdf: Res<DecorSdf>,
    buttons: Res<ButtonInput<MouseButton>>,
    fish: Query<(Entity, &Fish, &Transform)>,
    crabs: Query<(&Crab, &Transform)>,
    gliders: Query<(Entity, &Glider)>,
    stars: Query<(Entity, &Star)>,
    flakes: Query<(Entity, &Flake, &Transform)>,
    bubbles: Query<(Entity, &Bubble, &Transform)>,
) {
    out.frame += 1;
    let n = out.frame;
    let t = time.elapsed_secs();
    // Time step of the next frame.
    let dt = match out.dt_mode.as_str() {
        "30" => 1.0 / 30.0,
        // A window at ~45 fps: a third of the frames miss a refresh, and a
        // 100 ms hitch every ~5 s.
        "vsync" => match (n.wrapping_mul(2654435761) >> 7) % 300 {
            0 => 0.1,
            k if k % 3 == 0 => 1.0 / 30.0,
            _ => 1.0 / 60.0,
        },
        _ => 1.0 / 60.0,
    };
    *step = TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(dt));
    // Everything startles every 20 s (the crab runs for cover, flatfish lift off).
    if t >= out.next_startle {
        out.next_startle += 20.0;
        FORCE_STARTLE.store(true, std::sync::atomic::Ordering::Relaxed);
        writeln!(out.events.lock().unwrap(), "{n},{t:.4},startle").unwrap();
    }
    if buttons.just_pressed(MouseButton::Left) {
        writeln!(out.events.lock().unwrap(), "{n},{t:.4},click").unwrap();
    }
    if !sdf.complete {
        return;
    }
    let v = |p: Vec3| format!("{:.5},{:.5},{:.5}", p.x, p.y, p.z);

    let mut w = out.fish.lock().unwrap();
    for (e, f, tr) in &fish {
        writeln!(
            w,
            "{n},{t:.4},{},{},{:.4},{},{},{},{:.5},{:.5},{:.3},{:.3}",
            e.index(),
            f.species,
            f.length,
            v(tr.translation),
            v(tr.forward().as_vec3()),
            v(tr.up().as_vec3()),
            f.velocity.length(),
            sdf.distance(tr.translation),
            f.panic,
            f.satiety
        )
        .unwrap();
    }
    drop(w);

    let mut w = out.crab.lock().unwrap();
    for (c, tr) in &crabs {
        let p = c.probe(tr);
        let mut legs = String::new();
        for (foot, planted, hip, reach) in &p.feet {
            legs += &format!(
                ",{:.5},{},{:.3},{}",
                sdf.distance(*foot),
                *planted as u8,
                hip.distance(*foot) / reach,
                v(*foot)
            );
        }
        // Depth inside a rock (or the glass) of the carapace and the knees:
        // a point above the sand with a negative distance is in the decor.
        let rock = |pts: &[Vec3]| {
            pts.iter()
                .filter(|q| q.y > crate::scape::sand_height(q.x, q.z) + 0.001)
                .map(|q| (-sdf.distance(*q)).max(0.0))
                .fold(0.0f32, f32::max)
        };
        writeln!(
            w,
            "{n},{t:.4},{},{},{},{},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5}{legs}",
            p.state,
            v(p.contact),
            v(p.up),
            v(p.facing),
            p.speed,
            sdf.distance(p.contact),
            sdf.distance(p.centre),
            rock(&p.shell),
            rock(&p.knees[..8]),
            rock(&p.knees[8..])
        )
        .unwrap();
    }
    drop(w);

    let mut w = out.bottom.lock().unwrap();
    for (e, g) in &gliders {
        let (mode, pos, up, speed, buried) = g.probe();
        let above = pos.y - crate::scape::sand_height(pos.x, pos.z);
        writeln!(w, "{n},{t:.4},flatfish,{},{mode},{},{},{:.5},{:.5},{:.3},{above:.4}", e.index(), v(pos), v(up), speed, sdf.distance(pos), buried).unwrap();
    }
    for (e, s) in &stars {
        let (pos, up, speed) = s.probe();
        writeln!(w, "{n},{t:.4},starfish,{},crawl,{},{},{:.5},{:.5},0,0", e.index(), v(pos), v(up), speed, sdf.distance(pos)).unwrap();
    }
    drop(w);

    let mut w = out.flakes.lock().unwrap();
    for (e, f, tr) in &flakes {
        if let Some((state, vel)) = f.probe() {
            writeln!(w, "{n},{t:.4},{},{state},{},{},{:.5}", e.index(), v(tr.translation), v(vel), sdf.distance(tr.translation)).unwrap();
        }
    }
    drop(w);

    let mut w = out.bubbles.lock().unwrap();
    for (e, b, tr) in &bubbles {
        if let Some(vy) = b.probe() {
            writeln!(w, "{n},{t:.4},{},{:.5},{:.5}", e.index(), tr.translation.y, vy).unwrap();
        }
    }
}

//! Debug: `AQ_FISH_TRACE=file.csv` dumps every fish's pose each frame (with a
//! fixed 1/60 s time step) to analyse jerky motion offline.

use std::{fs::File, io::Write, sync::Mutex};

use bevy::{mesh::MeshTag, prelude::*, time::TimeUpdateStrategy};

use super::Fish;

#[derive(Resource)]
pub struct Trace(Mutex<File>, u32);

pub fn setup(app: &mut App) {
    let Ok(path) = std::env::var("AQ_FISH_TRACE") else {
        return;
    };
    let mut f = File::create(path).expect("trace file");
    writeln!(f, "frame,id,species,px,py,pz,fx,fy,fz,ux,uy,uz,speed,yaw_rate,tag,events").unwrap();
    app.insert_resource(Trace(Mutex::new(f), 0))
        .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)))
        .add_systems(Update, write.after(super::boids::animate));
}

fn write(mut trace: ResMut<Trace>, fish: Query<(Entity, &Fish, &Transform, &MeshTag)>) {
    trace.1 += 1;
    let frame = trace.1;
    let mut f = trace.0.lock().unwrap();
    for (e, fi, tr, tag) in &fish {
        let fw = tr.forward();
        let up = tr.up();
        let p = tr.translation;
        writeln!(
            f,
            "{frame},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.5},{},{}",
            e.index(),
            fi.species,
            p.x,
            p.y,
            p.z,
            fw.x,
            fw.y,
            fw.z,
            up.x,
            up.y,
            up.z,
            fi.velocity.length(),
            fi.yaw_rate,
            tag.0,
            fi.events
        )
        .unwrap();
    }
}

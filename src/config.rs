//! Command-line configuration.
//!
//! ```text
//! aquarium [--wallpaper [--no-sleep]] [--low-power] [--windowed] [--dof]
//!          [--quality low|medium|high] [--view fill|front|hero|side|top|low|close|inside]
//!          [--screenshot out.png] [--frames N]
//! ```

use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Low,
    Medium,
    High,
}

#[derive(Resource, Clone, Debug)]
pub struct AppConfig {
    /// 30 fps cap and half the particles.
    pub low_power: bool,
    /// Windowed 1600x1000 instead of borderless fullscreen.
    pub windowed: bool,
    /// Live desktop wallpaper: behind the desktop icons, click-through.
    pub wallpaper: bool,
    /// Wallpaper only: stay fully awake instead of the slow "sleeping aquarium".
    pub no_sleep: bool,
    /// Depth of field (off by default: it softens the image).
    pub dof: bool,
    pub quality: Quality,
    /// Fixed camera preset (disables the drift): fill, front, hero, side, top, low, close, inside.
    pub view: Option<String>,
    /// Capture the window to this file then exit.
    pub screenshot: Option<String>,
    /// Frames to render before taking the screenshot.
    pub frames: u32,
}

impl AppConfig {
    pub fn from_args() -> Self {
        let mut cfg = AppConfig {
            low_power: false,
            windowed: false,
            wallpaper: false,
            no_sleep: false,
            dof: false,
            quality: Quality::High,
            view: None,
            screenshot: None,
            frames: 90,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--low-power" => cfg.low_power = true,
                "--windowed" => cfg.windowed = true,
                "--wallpaper" => cfg.wallpaper = true,
                "--no-sleep" => cfg.no_sleep = true,
                "--dof" => cfg.dof = true,
                "--quality" => {
                    cfg.quality = match args.next().as_deref() {
                        Some("low") => Quality::Low,
                        Some("medium") => Quality::Medium,
                        _ => Quality::High,
                    }
                }
                "--view" => cfg.view = args.next(),
                "--screenshot" => {
                    cfg.screenshot = args.next();
                    cfg.windowed = true;
                }
                "--frames" => cfg.frames = args.next().and_then(|s| s.parse().ok()).unwrap_or(90),
                "-h" | "--help" => {
                    println!(
                        "aquarium — real-time 3D aquarium\n\n\
                         Options:\n  \
                         --wallpaper          live desktop wallpaper (behind the icons); sleeps\n  \
                                              in slow motion, wakes up when the mouse comes by\n  \
                         --no-sleep           wallpaper: always fully awake\n  \
                         --low-power          30 fps cap, half the particles\n  \
                         --windowed           run in a 1600x1000 window\n  \
                         --dof                depth of field (focus follows the cursor)\n  \
                         --quality <q>        low | medium | high (default high)\n  \
                         --view <v>           fixed camera: fill | front | hero | side | top | low | close | inside\n  \
                         --screenshot <file>  save a PNG after --frames frames, then exit\n  \
                         --frames <n>         frames before the screenshot (default 90)\n\n\
                         Mouse: move = the fish flee, left click = food, right drag = orbit,\n  \
                         wheel = zoom.  Keys: F3 debug overlay, Esc quit"
                    );
                    std::process::exit(0);
                }
                other => warn!("unknown argument {other}"),
            }
        }
        if cfg.low_power && cfg.quality == Quality::High {
            cfg.quality = Quality::Medium;
        }
        cfg
    }
}

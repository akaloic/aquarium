//! `--wallpaper`: turns the window into a live desktop background on macOS —
//! desktop window level (below the icons), covering the MacBook's own screen
//! (the main display when the lid is closed, followed as screens change),
//! click-through, on every Space, no Dock icon, declared as background (non
//! user-initiated) activity.
//! The power manager (`power.rs`) pauses it while the desktop is hidden and
//! lets it sleep in slow motion until the mouse comes by.

use bevy::{
    ecs::system::NonSendMarker,
    prelude::*,
    window::{PrimaryWindow, RawHandleWrapper, WindowOccluded},
};

use crate::config::AppConfig;

pub struct WallpaperPlugin;

impl Plugin for WallpaperPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Occluded>().add_systems(Update, install);
    }
}

/// True while the window is completely covered by other windows.
#[derive(Resource, Default)]
pub struct Occluded(pub bool);

/// Runs in the power manager's chain, right before its decision: when the
/// window shows up again, the very update woken by the event resumes rendering.
pub fn track_occlusion(mut events: MessageReader<WindowOccluded>, mut occluded: ResMut<Occluded>) {
    for e in events.read() {
        // `AQ_NO_OCCLUSION=1`: keep rendering behind other windows (measurements).
        occluded.0 = e.occluded && std::env::var("AQ_NO_OCCLUSION").is_err();
    }
}

fn install(
    config: Res<AppConfig>,
    time: Res<Time<Real>>,
    mut next_fit: Local<Option<f32>>,
    windows: Query<&RawHandleWrapper, With<PrimaryWindow>>,
    // AppKit must be called from the main thread.
    _main_thread: NonSendMarker,
) {
    if !config.wallpaper {
        return;
    }
    let Ok(handle) = windows.single() else {
        return;
    };
    let now = time.elapsed_secs();
    let first = next_fit.is_none();
    if next_fit.is_some_and(|t| now < t) {
        return;
    }
    // Screens come and go (lid closed, display plugged, scaling changed):
    // back onto the right one, at its size, every 2 s.
    *next_fit = Some(now + 2.0);
    #[cfg(target_os = "macos")]
    // SAFETY: running on the main thread (NonSendMarker), with a live window handle.
    unsafe {
        if first {
            macos::make_wallpaper(handle.get_window_handle());
            if !crate::macos::install_hotkey() {
                warn!("could not register the ⌃⌥⌘A hot key");
            }
            info!("wallpaper mode: ⌃⌥⌘A or the mouse on the desktop wakes it up; stop it with `pkill -x aquarium`");
        }
        if let Some(screen) = macos::fit_to_screen(handle.get_window_handle()) {
            info!("wallpaper: on {screen}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    if first {
        let _ = handle;
        warn!("--wallpaper is only implemented on macOS");
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::{class, msg_send, runtime::AnyObject};
    use objc2_foundation::NSRect;
    use raw_window_handle::RawWindowHandle;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWindowLevelForKey(key: i32) -> i32;
    }
    /// `kCGDesktopWindowLevelKey`: the level of the desktop picture, below the icons.
    const DESKTOP_WINDOW_LEVEL_KEY: i32 = 2;
    /// NSWindowCollectionBehavior: CanJoinAllSpaces | Stationary | IgnoresCycle.
    const COLLECTION_BEHAVIOR: usize = (1 << 0) | (1 << 4) | (1 << 6);
    /// NSApplicationActivationPolicyAccessory: no Dock icon, no menu bar.
    const ACTIVATION_POLICY_ACCESSORY: isize = 1;
    /// NSActivityBackground.
    const NS_ACTIVITY_BACKGROUND: u64 = 0xFF;

    unsafe fn ns_window(handle: RawWindowHandle) -> Option<*mut AnyObject> {
        let RawWindowHandle::AppKit(handle) = handle else {
            return None;
        };
        let view = handle.ns_view.as_ptr() as *mut AnyObject;
        let window: *mut AnyObject = unsafe { msg_send![view, window] };
        (!window.is_null()).then_some(window)
    }

    /// The MacBook's own screen when it is on, else the main display (lid
    /// closed), else any: not whichever screen was active at launch.
    unsafe fn pick_screen() -> Option<(*mut AnyObject, u32)> {
        unsafe {
            let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
            let count: usize = msg_send![screens, count];
            let key: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c"NSScreenNumber".as_ptr()];
            let (mut main, mut any) = (None, None);
            for i in 0..count {
                let screen: *mut AnyObject = msg_send![screens, objectAtIndex: i];
                let info: *mut AnyObject = msg_send![screen, deviceDescription];
                let number: *mut AnyObject = msg_send![info, objectForKey: key];
                if number.is_null() {
                    continue;
                }
                let display: u32 = msg_send![number, unsignedIntValue];
                if crate::macos::is_builtin(display) {
                    return Some((screen, display));
                }
                if display == crate::macos::main_display() {
                    main = Some((screen, display));
                }
                any.get_or_insert((screen, display));
            }
            main.or(any)
        }
    }

    /// Puts the window over the whole chosen screen; returns its name if the
    /// window had to move or resize.
    pub unsafe fn fit_to_screen(handle: RawWindowHandle) -> Option<String> {
        unsafe {
            let window = ns_window(handle)?;
            let (screen, display) = pick_screen()?;
            crate::macos::WALLPAPER_DISPLAY.store(display, std::sync::atomic::Ordering::Relaxed);
            let target: NSRect = msg_send![screen, frame];
            let current: NSRect = msg_send![window, frame];
            let same = (target.origin.x - current.origin.x).abs() < 0.5
                && (target.origin.y - current.origin.y).abs() < 0.5
                && (target.size.width - current.size.width).abs() < 0.5
                && (target.size.height - current.size.height).abs() < 0.5;
            if same {
                return None;
            }
            let _: () = msg_send![window, setFrame: target, display: true];
            let name: *mut AnyObject = msg_send![screen, localizedName];
            let name: *const std::ffi::c_char = if name.is_null() { std::ptr::null() } else { msg_send![name, UTF8String] };
            let name = if name.is_null() {
                "?".to_string()
            } else {
                std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned()
            };
            Some(format!("{name} ({:.0}×{:.0} points)", target.size.width, target.size.height))
        }
    }

    pub unsafe fn make_wallpaper(handle: RawWindowHandle) {
        unsafe {
            let Some(window) = ns_window(handle) else {
                return;
            };
            let level = CGWindowLevelForKey(DESKTOP_WINDOW_LEVEL_KEY) as isize;
            let _: () = msg_send![window, setLevel: level];
            let _: () = msg_send![window, setCollectionBehavior: COLLECTION_BEHAVIOR];
            let _: () = msg_send![window, setIgnoresMouseEvents: true];
            let _: () = msg_send![window, setHasShadow: false];
            let number: isize = msg_send![window, windowNumber];
            crate::macos::WALLPAPER_WINDOW.store(number as u32, std::sync::atomic::Ordering::Relaxed);
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let _: bool = msg_send![app, setActivationPolicy: ACTIVATION_POLICY_ACCESSORY];
            // Tell the system this is background work, not user-initiated
            // (App Nap and timer coalescing may apply). The token is kept forever.
            let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
            let reason: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: c"Aquarium live wallpaper".as_ptr()];
            let token: *mut AnyObject = msg_send![info, beginActivityWithOptions: NS_ACTIVITY_BACKGROUND, reason: reason];
            if !token.is_null() {
                let _: *mut AnyObject = msg_send![token, retain];
            }
        }
    }
}

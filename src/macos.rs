//! Thin macOS system bindings for the power manager: battery state and GPU
//! load (IOKit), thermal state and Low Power Mode (Foundation), screen lock /
//! display sleep and the global cursor (CoreGraphics), a global hot key
//! (Carbon), process priority. Everything here is thread-safe except
//! `install_hotkey` (main thread).

#![allow(non_upper_case_globals)]

use std::{
    ffi::{c_char, c_void},
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

type CFTypeRef = *const c_void;
type CFIndex = isize;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFArrayGetCount(a: CFTypeRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(a: CFTypeRef, i: CFIndex) -> CFTypeRef;
    fn CFDictionaryGetValue(d: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFNumberGetValue(n: CFTypeRef, kind: CFIndex, out: *mut c_void) -> bool;
    fn CFStringCreateWithCString(alloc: CFTypeRef, s: *const c_char, encoding: u32) -> CFTypeRef;
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> bool;
    fn CFBooleanGetValue(b: CFTypeRef) -> bool;
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CFTypeRef;
    fn IOServiceGetMatchingServices(main_port: u32, matching: CFTypeRef, existing: *mut u32) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IORegistryEntryCreateCFProperty(entry: u32, key: CFTypeRef, alloc: CFTypeRef, options: u32) -> CFTypeRef;
    fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
    fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFTypeRef;
    fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef) -> CFTypeRef;
    fn IOPSGetProvidingPowerSourceType(blob: CFTypeRef) -> CFTypeRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreate(source: CFTypeRef) -> CFTypeRef;
    fn CGEventGetLocation(event: CFTypeRef) -> CGPoint;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
    fn CGRectMakeWithDictionaryRepresentation(dict: CFTypeRef, rect: *mut CGRect) -> bool;
    fn CGSessionCopyCurrentDictionary() -> CFTypeRef;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayIsAsleep(display: u32) -> u32;
    fn CGDisplayIsBuiltin(display: u32) -> u32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    static kCGWindowLayer: CFTypeRef;
    static kCGWindowBounds: CFTypeRef;
    static kCGWindowAlpha: CFTypeRef;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32,
        num_types: usize,
        list: *const EventTypeSpec,
        user_data: *mut c_void,
        out_ref: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: *mut c_void,
        options: u32,
        out_ref: *mut *mut c_void,
    ) -> i32;
}

unsafe extern "C" {
    fn setpriority(which: i32, who: u32, prio: i32) -> i32;
}

const UTF8: u32 = 0x0800_0100;
const CF_NUMBER_SINT64: CFIndex = 4;
const CF_NUMBER_FLOAT64: CFIndex = 6;

/// Owned CFString from a Rust literal.
struct CfStr(CFTypeRef);

impl CfStr {
    fn new(s: &str) -> Self {
        let c = std::ffi::CString::new(s).unwrap();
        Self(unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) })
    }
}

impl Drop for CfStr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

fn number(n: CFTypeRef) -> Option<f64> {
    if n.is_null() {
        return None;
    }
    let mut v = 0.0f64;
    unsafe { CFNumberGetValue(n, CF_NUMBER_FLOAT64, &mut v as *mut f64 as *mut c_void) }.then_some(v)
}

fn int(n: CFTypeRef) -> Option<i64> {
    if n.is_null() {
        return None;
    }
    let mut v = 0i64;
    unsafe { CFNumberGetValue(n, CF_NUMBER_SINT64, &mut v as *mut i64 as *mut c_void) }.then_some(v)
}

/// Power source: (on battery, charge 0..1).
pub fn battery() -> (bool, Option<f32>) {
    unsafe {
        let blob = IOPSCopyPowerSourcesInfo();
        if blob.is_null() {
            return (false, None);
        }
        let on_battery = {
            let kind = IOPSGetProvidingPowerSourceType(blob);
            let battery = CfStr::new("Battery Power");
            !kind.is_null() && CFEqual(kind, battery.0)
        };
        let mut charge = None;
        let list = IOPSCopyPowerSourcesList(blob);
        if !list.is_null() {
            let (cur_key, max_key) = (CfStr::new("Current Capacity"), CfStr::new("Max Capacity"));
            for i in 0..CFArrayGetCount(list) {
                let desc = IOPSGetPowerSourceDescription(blob, CFArrayGetValueAtIndex(list, i));
                if desc.is_null() {
                    continue;
                }
                let cur = number(CFDictionaryGetValue(desc, cur_key.0));
                let max = number(CFDictionaryGetValue(desc, max_key.0));
                if let (Some(c), Some(m)) = (cur, max) {
                    if m > 0.0 {
                        charge = Some((c / m) as f32);
                    }
                }
            }
            CFRelease(list);
        }
        CFRelease(blob);
        (on_battery, charge)
    }
}

/// How busy the GPU has been lately (0..1, all processes: the driver's own
/// "Device Utilization %", what Activity Monitor shows). No permission needed.
pub fn gpu_busy() -> Option<f32> {
    unsafe {
        let mut iter = 0u32;
        if IOServiceGetMatchingServices(0, IOServiceMatching(c"IOAccelerator".as_ptr()), &mut iter) != 0 {
            return None;
        }
        let stats_key = CfStr::new("PerformanceStatistics");
        let busy_key = CfStr::new("Device Utilization %");
        let mut busy = None;
        loop {
            let entry = IOIteratorNext(iter);
            if entry == 0 {
                break;
            }
            let stats = IORegistryEntryCreateCFProperty(entry, stats_key.0, std::ptr::null(), 0);
            if !stats.is_null() {
                if let Some(v) = int(CFDictionaryGetValue(stats, busy_key.0)) {
                    busy = Some(busy.unwrap_or(0.0f32).max(v as f32 / 100.0));
                }
                CFRelease(stats);
            }
            IOObjectRelease(entry);
        }
        IOObjectRelease(iter);
        busy
    }
}

/// `NSProcessInfo.thermalState`: 0 nominal, 1 fair, 2 serious, 3 critical.
pub fn thermal_state() -> u8 {
    use objc2::{class, msg_send, runtime::AnyObject};
    // SAFETY: a class method and a property getter, thread-safe.
    unsafe {
        let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        let state: isize = msg_send![info, thermalState];
        state.clamp(0, 3) as u8
    }
}

/// Low Power Mode (System Settings > Battery).
pub fn low_power_mode() -> bool {
    use objc2::{class, msg_send, runtime::AnyObject};
    // SAFETY: as above.
    unsafe {
        let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        msg_send![info, isLowPowerModeEnabled]
    }
}

/// Display the wallpaper covers (0 until installed, or outside wallpaper mode).
pub static WALLPAPER_DISPLAY: AtomicU32 = AtomicU32::new(0);

pub fn main_display() -> u32 {
    unsafe { CGMainDisplayID() }
}

/// The MacBook's own screen.
pub fn is_builtin(display: u32) -> bool {
    unsafe { CGDisplayIsBuiltin(display) != 0 }
}

/// Where the wallpaper is, in global points (top-left origin, like the cursor).
pub fn wallpaper_bounds() -> Option<(f64, f64, f64, f64)> {
    let display = WALLPAPER_DISPLAY.load(Ordering::Relaxed);
    if display == 0 {
        return None;
    }
    let r = unsafe { CGDisplayBounds(display) };
    (r.w > 0.0 && r.h > 0.0).then_some((r.x, r.y, r.w, r.h))
}

/// Screen locked, fast-user-switched away, or display asleep (the wallpaper's
/// display, else the main one).
pub fn screen_unavailable() -> bool {
    unsafe {
        let display = match WALLPAPER_DISPLAY.load(Ordering::Relaxed) {
            0 => CGMainDisplayID(),
            d => d,
        };
        if CGDisplayIsAsleep(display) != 0 {
            return true;
        }
        let dict = CGSessionCopyCurrentDictionary();
        if dict.is_null() {
            // No window-server session (e.g. ssh): nothing is visible anyway.
            return false;
        }
        let locked_key = CfStr::new("CGSSessionScreenIsLocked");
        let console_key = CfStr::new("kCGSSessionOnConsoleKey");
        let locked = CFDictionaryGetValue(dict, locked_key.0);
        let on_console = CFDictionaryGetValue(dict, console_key.0);
        let unavailable = (!locked.is_null() && CFBooleanGetValue(locked))
            || (!on_console.is_null() && !CFBooleanGetValue(on_console));
        CFRelease(dict);
        unavailable
    }
}

/// Global cursor position (points, origin at the top-left of the main display).
pub fn cursor() -> Option<(f64, f64)> {
    unsafe {
        let ev = CGEventCreate(std::ptr::null());
        if ev.is_null() {
            return None;
        }
        let p = CGEventGetLocation(ev);
        CFRelease(ev);
        Some((p.x, p.y))
    }
}

/// CGWindowID of the wallpaper window (set once it is installed).
pub static WALLPAPER_WINDOW: AtomicU32 = AtomicU32::new(0);

/// Whether the point (global, top-left origin) shows the desktop, i.e. no
/// window, panel, menu bar or Dock above the wallpaper covers it. Needs no
/// permission (only window bounds and layers are read).
pub fn over_desktop(x: f64, y: f64) -> bool {
    const ON_SCREEN_ABOVE_WINDOW: u32 = 1 << 1;
    const EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;
    let ours = WALLPAPER_WINDOW.load(Ordering::Relaxed);
    if ours == 0 {
        return false;
    }
    // The desktop of another screen shows no aquarium.
    if !wallpaper_bounds().is_some_and(|(bx, by, bw, bh)| x >= bx && x < bx + bw && y >= by && y < by + bh) {
        return false;
    }
    unsafe {
        let list = CGWindowListCopyWindowInfo(ON_SCREEN_ABOVE_WINDOW | EXCLUDE_DESKTOP_ELEMENTS, ours);
        if list.is_null() {
            return false;
        }
        let mut covered = false;
        for i in 0..CFArrayGetCount(list) {
            let w = CFArrayGetValueAtIndex(list, i);
            // Invisible overlays (alpha 0) don't hide anything.
            if number(CFDictionaryGetValue(w, kCGWindowAlpha)).is_some_and(|a| a < 0.01) {
                continue;
            }
            // The desktop icons layer and below are part of the desktop.
            if int(CFDictionaryGetValue(w, kCGWindowLayer)).is_some_and(|l| l < 0) {
                continue;
            }
            let mut r = CGRect::default();
            let bounds = CFDictionaryGetValue(w, kCGWindowBounds);
            if !bounds.is_null()
                && CGRectMakeWithDictionaryRepresentation(bounds, &mut r)
                && x >= r.x
                && x < r.x + r.w
                && y >= r.y
                && y < r.y + r.h
            {
                covered = true;
                break;
            }
        }
        CFRelease(list);
        !covered
    }
}

/// Lowers the whole process to the "Darwin background" band (efficiency cores,
/// throttled I/O and timers) or restores it.
pub fn set_background_priority(background: bool) {
    const PRIO_DARWIN_PROCESS: i32 = 4;
    const PRIO_DARWIN_BG: i32 = 0x1000;
    unsafe {
        setpriority(PRIO_DARWIN_PROCESS, 0, if background { PRIO_DARWIN_BG } else { 0 });
    }
}

// --- Global hot key: ⌃⌥⌘A wakes the aquarium up -------------------------------

#[repr(C)]
struct EventTypeSpec {
    class: u32,
    kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

pub static HOTKEY_PRESSED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_hotkey(_: *mut c_void, _: *mut c_void, _: *mut c_void) -> i32 {
    HOTKEY_PRESSED.store(true, Ordering::Relaxed);
    0
}

/// Registers ⌃⌥⌘A (no Accessibility permission needed with Carbon hot keys).
/// Main thread only.
pub fn install_hotkey() -> bool {
    const KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
    const HOTKEY_PRESSED_KIND: u32 = 5;
    const KEY_A: u32 = 0;
    const CMD: u32 = 1 << 8;
    const OPTION: u32 = 1 << 11;
    const CONTROL: u32 = 1 << 12;
    unsafe {
        let target = GetApplicationEventTarget();
        let spec = EventTypeSpec {
            class: KEYBOARD,
            kind: HOTKEY_PRESSED_KIND,
        };
        let mut handler = std::ptr::null_mut();
        if InstallEventHandler(target, on_hotkey, 1, &spec, std::ptr::null_mut(), &mut handler) != 0 {
            return false;
        }
        let id = EventHotKeyId {
            signature: u32::from_be_bytes(*b"AQUA"),
            id: 1,
        };
        let mut hotkey = std::ptr::null_mut();
        RegisterEventHotKey(KEY_A, CMD | OPTION | CONTROL, id, target, 0, &mut hotkey) == 0
    }
}

// Mabel desktop companion — the cat that occasionally walks across your screen.
//
// Architecture: a separate Tauri WebviewWindow ("companion") hosts
// src/companion.html. The window is transparent, decoration-less,
// always-on-top, click-through, skip-taskbar. The Rust side does ALL window
// movement via window.set_position in a 60Hz tokio loop — much more reliable
// than trying to call Tauri APIs from the companion's webview JS, which has a
// limited API surface. The JS side handles only the cosmetic CSS state
// (facing direction, sit pose, blink), driven by a single
// `mabel-companion-state` event.
//
// Visit lifecycle:
//   1. Pick a screen + entry edge (left or right).
//   2. Compute walk path along the bottom strip of the screen.
//   3. Show the window off-screen at the start point.
//   4. Tween the window position to the midpoint over half the visit.
//   5. Optional sit + blink for ~3s.
//   6. Tween from midpoint to off-screen on the opposite edge.
//   7. Hide the window.
//
// Art: currently uses public/mabel.png (the app icon) with a CSS bob to fake
// walking. When real sprite frames land, the rendering swaps to a sprite-cycle
// inside the same window — no Rust changes needed (see F9).

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

use crate::settings::Settings;

/// True while a visit is in progress. Lets a second `companion_visit_now` call
/// cancel the in-flight visit instead of starting a parallel one. The tween
/// loop reads this each frame and bails out cleanly when it flips to false.
static VISIT_ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
struct CompanionState {
    facing: &'static str,
    sitting: bool,
    blink: bool,
}

fn size_for(setting: &str, screen_height: f64) -> u32 {
    let pct = match setting {
        "small" => 0.08,
        "large" => 0.20,
        _ => 0.14, // medium / default
    };
    (screen_height * pct).round() as u32
}

fn visit_plan(setting: &str) -> (Duration, bool) {
    match setting {
        "short" => (Duration::from_secs(10), false),
        "long" => (Duration::from_secs(60), true),
        _ => (Duration::from_secs(30), true),
    }
}

pub fn frequency_for(setting: &str) -> Duration {
    match setting {
        "15min" => Duration::from_secs(15 * 60),
        "1hr" => Duration::from_secs(60 * 60),
        "2hr" => Duration::from_secs(2 * 60 * 60),
        _ => Duration::from_secs(30 * 60),
    }
}

/// Tween the companion window between two logical positions over `duration`.
/// Updates at ~60Hz. Blocking on the calling task.
async fn tween_position(
    window: &tauri::WebviewWindow,
    start_logical: (f64, f64),
    end_logical: (f64, f64),
    scale: f64,
    duration: Duration,
) {
    let frame_dur = Duration::from_millis(16);
    let total_frames = (duration.as_millis() / 16).max(1) as u64;
    for frame in 0..=total_frames {
        if !VISIT_ACTIVE.load(Ordering::Relaxed) {
            return;
        }
        let t = frame as f64 / total_frames as f64;
        // Slight ease at the start and end so she doesn't snap.
        let eased = if t < 0.1 {
            (t / 0.1) * 0.1
        } else if t > 0.9 {
            0.9 + ((t - 0.9) / 0.1) * 0.1
        } else {
            t
        };
        let lx = start_logical.0 + (end_logical.0 - start_logical.0) * eased;
        let ly = start_logical.1 + (end_logical.1 - start_logical.1) * eased;
        let _ = window.set_position(PhysicalPosition::new(
            (lx * scale).round() as i32,
            (ly * scale).round() as i32,
        ));
        tokio::time::sleep(frame_dur).await;
    }
}

/// Cancel any in-flight visit. Returns true if one was running. Called by the
/// frontend toggle to support "click again to dismiss".
pub fn cancel_visit() -> bool {
    VISIT_ACTIVE.swap(false, Ordering::Relaxed)
}

/// Whether a visit is currently in progress.
pub fn is_visiting() -> bool {
    VISIT_ACTIVE.load(Ordering::Relaxed)
}

pub async fn run_visit(app: &AppHandle, settings: &Settings) {
    if VISIT_ACTIVE.swap(true, Ordering::Relaxed) {
        // Another visit is already running. The toggle command takes care of
        // cancellation; here we just bail to avoid stacking.
        return;
    }
    println!("[Mabel] companion visit starting");
    let window = match app.get_webview_window("companion") {
        Some(w) => w,
        None => {
            eprintln!("[Mabel] companion window not found, skipping visit");
            return;
        }
    };

    let monitor = match app.primary_monitor() {
        Ok(Some(m)) => m,
        _ => {
            eprintln!("[Mabel] no primary monitor, skipping visit");
            return;
        }
    };
    let screen = monitor.size();
    let scale = monitor.scale_factor();
    let monitor_origin = monitor.position();
    // logical_w/h are in points. monitor_origin is in physical pixels and may
    // be non-zero on multi-display setups (the primary isn't always at 0,0).
    let logical_w = screen.width as f64 / scale;
    let logical_h = screen.height as f64 / scale;
    let origin_x_logical = monitor_origin.x as f64 / scale;
    let origin_y_logical = monitor_origin.y as f64 / scale;

    let size_logical = size_for(&settings.companion_size, logical_h) as f64;
    let _ = window.set_size(PhysicalSize::new(
        (size_logical * scale).round() as u32,
        (size_logical * scale).round() as u32,
    ));

    let (visit_dur, do_sit) = visit_plan(&settings.companion_visit);

    // Pick entry side (poor man's randomness — good enough).
    let from_left = chrono::Utc::now().timestamp_millis() % 2 == 0;
    // Position her in the lower middle band — comfortably on-screen, below any
    // typical content but well above the dock. Never goes off the bottom edge.
    let walk_y = origin_y_logical + logical_h * 0.65;
    // Start her ON-screen at the edge (not off-screen). macOS may clamp windows
    // that try to position outside the visible monitor bounds, so off-screen
    // entry can silently fail. Edge entry still feels like she walked in.
    let left_x = origin_x_logical;
    let right_x = origin_x_logical + logical_w - size_logical;
    let (start_x, end_x, facing) = if from_left {
        (left_x, right_x, "right")
    } else {
        (right_x, left_x, "left")
    };

    println!(
        "[Mabel] companion screen: logical {}x{} @ origin ({},{}), scale {}, size {}, walk_y {}",
        logical_w as i32, logical_h as i32,
        origin_x_logical as i32, origin_y_logical as i32,
        scale, size_logical as i32, walk_y as i32,
    );

    // Pre-position before showing so she doesn't flash at (0, 0).
    let _ = window.set_position(PhysicalPosition::new(
        (start_x * scale).round() as i32,
        (walk_y * scale).round() as i32,
    ));
    let _ = app.emit_to(
        "companion",
        "mabel-companion-state",
        CompanionState { facing, sitting: false, blink: false },
    );
    // Belt-and-suspenders: also push the facing direction via direct JS eval,
    // because the event-listener path has not been reliably mirroring the
    // sprite. eval runs in the companion's webview the moment we call it; the
    // worst case is it's a no-op if the page hasn't finished loading yet.
    let scale_x = if facing == "left" { -1 } else { 1 };
    let _ = window.eval(&format!(
        "(function(){{const s=document.getElementById('stage');if(s){{s.style.transform='scaleX({})';}}}}());",
        scale_x
    ));
    if let Err(e) = window.show() {
        eprintln!("[Mabel] companion show failed: {}", e);
    }
    let _ = window.set_always_on_top(true);
    println!("[Mabel] companion shown at logical ({},{})", start_x as i32, walk_y as i32);

    let mid_x = (start_x + end_x) / 2.0;
    let half_dur = visit_dur / 2;

    println!(
        "[Mabel] companion walking from {:.0} to {:.0} at y={:.0}, scale={}",
        start_x, mid_x, walk_y, scale
    );
    tween_position(
        &window,
        (start_x, walk_y),
        (mid_x, walk_y),
        scale,
        half_dur,
    )
    .await;

    if do_sit && VISIT_ACTIVE.load(Ordering::Relaxed) {
        // Sit pose: kill the animation outright (animationPlayState alone is
        // not enough to override a running CSS animation shorthand) and lock
        // the sprite to frame 7 of 8 (-600vw). Setting !important via cssText
        // so any concurrent CSS keyframe transform write loses.
        let _ = window.eval(
            "(function(){const s=document.getElementById('sprite');if(s){s.style.cssText='position:absolute;top:0;left:0;width:800vw;height:100vh;animation:none !important;transform:translateX(-600vw) !important;';}})();",
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
        // Blink: same lock, frame 8 (-700vw).
        let _ = window.eval(
            "(function(){const s=document.getElementById('sprite');if(s){s.style.cssText='position:absolute;top:0;left:0;width:800vw;height:100vh;animation:none !important;transform:translateX(-700vw) !important;';}})();",
        );
        tokio::time::sleep(Duration::from_millis(180)).await;
        // Eyes back open.
        let _ = window.eval(
            "(function(){const s=document.getElementById('sprite');if(s){s.style.cssText='position:absolute;top:0;left:0;width:800vw;height:100vh;animation:none !important;transform:translateX(-600vw) !important;';}})();",
        );
        tokio::time::sleep(Duration::from_millis(1820)).await;
        // Resume walking: clear inline style so the CSS keyframe rule kicks
        // back in.
        let _ = window.eval(
            "(function(){const s=document.getElementById('sprite');if(s){s.style.cssText='';}})();",
        );
    }

    tween_position(&window, (mid_x, walk_y), (end_x, walk_y), scale, half_dur).await;

    // Park the window off-screen instead of hiding it. NSWindow show/hide
    // cycles are unreliable on transparent windows in our experience — windows
    // hidden once won't always re-render content on subsequent show(). Parking
    // off-screen sidesteps that entirely.
    let _ = window.set_position(PhysicalPosition::new(-9999, -9999));
    VISIT_ACTIVE.store(false, Ordering::Relaxed);
    println!("[Mabel] companion visit done");
}

pub fn spawn_scheduler(app: AppHandle, settings_handle: Arc<Mutex<Settings>>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let snapshot = {
                let s = settings_handle.lock().unwrap();
                s.clone()
            };
            let interval = frequency_for(&snapshot.companion_frequency);
            tokio::time::sleep(interval).await;

            let snapshot = {
                let s = settings_handle.lock().unwrap();
                s.clone()
            };
            if snapshot.companion_enabled {
                run_visit(&app, &snapshot).await;
            }
        }
    });
}

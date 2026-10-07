//! Developer hooks, driven by environment variables (unused in normal runs):
//!
//! - `NEVIS_BENCH=1`: print how long it took until the file was on screen, then quit.
//!   With `NEVIS_T0=$(date +%s%N)` the time before `main()` (exec, linking) is included.
//! - `NEVIS_SNAPSHOT=out.png`: save a picture of the window once it is drawn, then quit.
//! - `NEVIS_ACTIONS=win.reading,win.find`: actions to run first (for the picture).

use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gtk::{glib, prelude::*};

static START: OnceLock<Instant> = OnceLock::new();

pub fn mark_start() {
    START.get_or_init(Instant::now);
}

pub fn active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| {
        std::env::var_os("NEVIS_BENCH").is_some() || std::env::var_os("NEVIS_SNAPSHOT").is_some()
    })
}

/// With `NEVIS_BENCH` set, prints how far into startup `label` was reached.
pub fn lap(label: &str) {
    if active() {
        let ms = START.get().map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0);
        eprintln!("  {ms:8.1} ms  {label}");
    }
}

/// Called once the window's content is in place; reports after the next painted frame.
pub fn content_ready(window: &gtk::ApplicationWindow) {
    // NEVIS_TRACE=1 on a long-running instance: print when each window's text is on screen
    // (ns since the epoch, to compare with the launching shell's `date +%s%N`).
    if std::env::var_os("NEVIS_TRACE").is_some() {
        crate::editor::after_next_frame(window, || {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
            eprintln!("nevis: text on screen at {now}");
        });
    }
    if !active() {
        return;
    }
    let target = window.clone();
    crate::editor::after_next_frame(window, move || report(target));
}

fn report(window: gtk::ApplicationWindow) {
    if std::env::var_os("NEVIS_BENCH").is_some() {
        let in_main = START.get().map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0);
        let since_launch = std::env::var("NEVIS_T0")
            .ok()
            .and_then(|t0| t0.trim().parse::<u128>().ok())
            .and_then(|t0| SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_nanos().checked_sub(t0))
            .map(|ns| ns as f64 / 1e6);
        match since_launch {
            Some(ms) => eprintln!("nevis: text on screen {ms:.1} ms after launch ({in_main:.1} ms in main)"),
            None => eprintln!("nevis: text on screen {in_main:.1} ms after main()"),
        }
    }
    // NEVIS_ACTIONS=win.reading,win.shortcuts: put the window in a state worth a picture.
    if let Ok(actions) = std::env::var("NEVIS_ACTIONS") {
        for action in actions.split(',').filter(|a| !a.is_empty()) {
            if WidgetExt::activate_action(&window, action, None).is_err() {
                eprintln!("nevis: no action {action}");
            }
        }
    }
    // Give deferred work (highlighting) time to land before the picture and the exit.
    glib::timeout_add_local_once(Duration::from_millis(500), move || {
        if let Some(path) = std::env::var_os("NEVIS_SNAPSHOT")
            && let Err(err) = snapshot(&window, &path)
        {
            eprintln!("nevis: snapshot failed: {err}");
        }
        if let Some(app) = window.application() {
            app.quit();
        }
    });
}

fn snapshot(window: &gtk::ApplicationWindow, path: &std::ffi::OsStr) -> Result<(), String> {
    let content = window.child().ok_or("window has no content")?;
    let snap = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(&content)).snapshot(&snap, content.width().into(), content.height().into());
    let node = snap.to_node().ok_or("nothing was drawn")?;
    let renderer = window.renderer().ok_or("window has no renderer")?;
    renderer.render_texture(&node, None).save_to_png(path).map_err(|e| e.to_string())
}

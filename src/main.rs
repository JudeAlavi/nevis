//! nevis: a fast, minimal text and Markdown editor. One file per window.

mod devhooks;
mod editor;
mod find;
mod help;
mod render;
mod theme;

use gtk::{gio, glib, prelude::*};

const APP_ID: &str = "dev.shirvani.Nevis";

fn main() -> glib::ExitCode {
    devhooks::mark_start();

    // One process serves every window, so opening a second file skips GTK startup.
    // Measurement runs get their own process instead of handing off to a running one.
    let mut flags = gio::ApplicationFlags::HANDLES_OPEN;
    if devhooks::active() {
        flags |= gio::ApplicationFlags::NON_UNIQUE;
    }
    let app = gtk::Application::builder().application_id(APP_ID).flags(flags).build();

    app.connect_startup(|app| {
        devhooks::lap("gtk ready");
        sourceview::init();
        theme::init();
        devhooks::lap("theme ready");
        install_actions(app);
        app.connect_window_removed(|_, window| editor::forget(window));
        // Started as a background service (`nevis --gapplication-service`, see nevis.service):
        // stay running with no windows, so every file opens without GTK's ~0.5 s startup.
        if app.flags().contains(gio::ApplicationFlags::IS_SERVICE) {
            std::mem::forget(app.hold());
        }
    });
    app.connect_activate(|app| editor::Editor::open(app, None));
    app.connect_open(|app, files, _hint| {
        for file in files {
            editor::Editor::open(app, Some(file.clone()));
        }
    });
    app.run()
}

fn install_actions(app: &gtk::Application) {
    let new_window = gio::SimpleAction::new("new-window", None);
    let weak = app.downgrade();
    new_window.connect_activate(move |_, _| {
        if let Some(app) = weak.upgrade() {
            editor::Editor::open(&app, None);
        }
    });
    app.add_action(&new_window);

    // Closing each window lets it ask about unsaved changes first.
    let quit = gio::SimpleAction::new("quit", None);
    let weak = app.downgrade();
    quit.connect_activate(move |_, _| {
        if let Some(app) = weak.upgrade() {
            for window in app.windows() {
                window.close();
            }
        }
    });
    app.add_action(&quit);

    for (action, keys) in [
        ("app.new-window", &["<Ctrl>n"][..]),
        ("app.quit", &["<Ctrl>q"]),
        ("win.open", &["<Ctrl>o"]),
        ("win.save", &["<Ctrl>s"]),
        ("win.save-as", &["<Ctrl><Shift>s"]),
        ("win.close", &["<Ctrl>w"]),
        ("win.find", &["<Ctrl>f"]),
        ("win.replace", &["<Ctrl>h"]),
        ("win.find-next", &["F3", "<Ctrl>g"]),
        ("win.find-prev", &["<Shift>F3", "<Ctrl><Shift>g"]),
        ("win.zoom-in", &["<Ctrl>plus", "<Ctrl>equal", "<Ctrl>KP_Add"]),
        ("win.zoom-out", &["<Ctrl>minus", "<Ctrl>KP_Subtract"]),
        ("win.zoom-reset", &["<Ctrl>0", "<Ctrl>KP_0"]),
        ("win.wrap", &["<Alt>z"]),
        ("win.reading", &["<Ctrl>e"]),
        ("win.line-numbers", &["<Ctrl><Shift>l"]),
        ("win.shortcuts", &["<Ctrl>question", "F1"]),
    ] {
        app.set_accels_for_action(action, keys);
    }
}

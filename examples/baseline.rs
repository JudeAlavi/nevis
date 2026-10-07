//! Startup floor: the smallest GTK4 window, timed from main() to its first painted frame.
//! Run with `cargo run --release --example baseline`.

use std::time::Instant;

use gtk::{glib, prelude::*};

fn main() -> glib::ExitCode {
    let start = Instant::now();
    let app = gtk::Application::builder()
        .application_id("dev.shirvani.NevisBaseline")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(move |_| eprintln!("  {:8.1} ms  gtk ready", start.elapsed().as_secs_f64() * 1000.0));
    app.connect_activate(move |app| {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .default_width(860)
            .default_height(640)
            .child(&gtk::Label::new(Some("hello")))
            .build();
        window.present();
        eprintln!("  {:8.1} ms  presented", start.elapsed().as_secs_f64() * 1000.0);
        let clock = window.frame_clock().expect("realized window");
        let app = app.clone();
        clock.connect_after_paint(move |_| {
            eprintln!("  {:8.1} ms  first frame", start.elapsed().as_secs_f64() * 1000.0);
            app.quit();
        });
    });
    app.run()
}

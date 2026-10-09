//! `nevis`: hands files to the running nevis over D-Bus without loading GTK, so opening a
//! file costs a few milliseconds instead of ~100. With no nevis running (or for options and
//! dev runs) it starts the full app, `nevis-app`, in its place.

use std::os::unix::process::CommandExt;
use std::process::Command;

use gio::glib;
use gio::prelude::*;

const APP_ID: &str = "dev.shirvani.Nevis";

/// Dev runs (timing, pictures) need their own app process, not the running service.
const OWN_PROCESS: [&str; 2] = ["NEVIS_BENCH", "NEVIS_SNAPSHOT"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = args.iter().any(|arg| arg.starts_with('-'));
    let dev = OWN_PROCESS.iter().any(|var| std::env::var_os(var).is_some());
    if !options && !dev && hand_over(&args).is_ok() {
        return;
    }
    let app = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("nevis-app")))
        .filter(|path| path.exists())
        .unwrap_or_else(|| "nevis-app".into());
    let error = Command::new(app).args(&args).exec();
    eprintln!("nevis: could not start nevis-app: {error}");
    std::process::exit(1);
}

/// Asks the running nevis to open `args` (or a new window); fails if none is running.
fn hand_over(args: &[String]) -> Result<(), glib::Error> {
    let app_id = std::env::var("NEVIS_APP_ID").unwrap_or_else(|_| APP_ID.to_owned());
    let object = format!("/{}", app_id.replace('.', "/"));
    let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)?;

    // Lets the compositor give the new window focus, as if the app had opened it itself.
    let platform_data = glib::VariantDict::new(None);
    for (key, variable) in [("activation-token", "XDG_ACTIVATION_TOKEN"), ("desktop-startup-id", "DESKTOP_STARTUP_ID")] {
        if let Ok(value) = std::env::var(variable) {
            platform_data.insert(key, value);
        }
    }
    let (method, parameters) = if args.is_empty() {
        ("Activate", glib::Variant::tuple_from_iter([platform_data.end()]))
    } else {
        let uris: Vec<String> = args.iter().map(|arg| gio::File::for_commandline_arg(arg).uri().to_string()).collect();
        ("Open", glib::Variant::tuple_from_iter([uris.to_variant(), platform_data.end()]))
    };
    bus.call_sync(
        Some(&app_id),
        &object,
        "org.freedesktop.Application",
        method,
        Some(&parameters),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        2000,
        None::<&gio::Cancellable>,
    )?;
    Ok(())
}

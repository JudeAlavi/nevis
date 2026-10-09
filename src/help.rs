//! The keyboard shortcut cheat sheet (Ctrl+? or F1), drawn over the window.
//!
//! Key labels come from the app's real bindings, so the sheet can't drift from them.

use gtk::prelude::*;

struct Row {
    what: &'static str,
    /// Action whose bindings are listed first ("" for none).
    action: &'static str,
    /// Keys GTK or the find bar handle themselves, listed after the action's.
    also: &'static str,
}

const fn row(what: &'static str, action: &'static str, also: &'static str) -> Row {
    Row { what, action, also }
}

const LEFT: &[(&str, &[Row])] = &[
    ("File", &[
        row("New note (named, in a folder window)", "win.new-note", ""),
        row("New tab", "win.new-tab", ""),
        row("New window", "app.new-window", ""),
        row("Open a file", "win.open", ""),
        row("Open a folder", "win.open-folder", ""),
        row("Go to a file, heading or #tag", "win.quick-open", ""),
        row("Search in the folder", "win.search", ""),
        row("Save", "win.save", ""),
        row("Save as", "win.save-as", ""),
        row("Close tab", "win.close", ""),
        row("Reopen closed tab", "win.reopen-tab", ""),
        row("Close window", "win.close-window", ""),
        row("Quit, keeping every tab", "app.quit", ""),
    ]),
    ("Tabs", &[
        row("Next / previous tab", "win.next-tab", "Ctrl+Shift+Tab"),
        row("Tab 1 … 8, last tab", "win.tab-1", "… Alt+9"),
        row("Move tab left / right", "win.move-tab-left", "Ctrl+Shift+Page Down"),
    ]),
    ("Find", &[
        row("Find", "win.find", ""),
        row("Replace", "win.replace", ""),
        row("Next match", "win.find-next", "Enter"),
        row("Previous match", "win.find-prev", "Shift+Enter"),
        row("Close the find bar", "", "Esc"),
    ]),
    ("Help", &[
        row("This cheat sheet", "win.shortcuts", ""),
        row("Menu", "", "Right-click / Shift+F10"),
    ]),
];

const RIGHT: &[(&str, &[Row])] = &[
    ("View", &[
        row("Rendered view ⇄ source", "win.reading", ""),
        row("Folder sidebar", "win.sidebar", ""),
        row("Settings (theme, text size…)", "win.settings", ""),
        row("Line numbers", "win.line-numbers", ""),
        row("Word wrap", "win.wrap", ""),
        row("Larger text", "win.zoom-in", "Ctrl+scroll"),
        row("Smaller text", "win.zoom-out", ""),
        row("Default text size", "win.zoom-reset", ""),
    ]),
    ("Notes", &[
        row("Go to a heading", "win.outline", ""),
        row("Today's note (in a folder window)", "win.daily", ""),
        row("Make a link (or paste an address over words)", "win.link", ""),
        row("Paste a screenshot (saved in attachments/)", "", "Ctrl+V"),
        row("Complete [[links]], #tags, > [!callouts]", "", "Enter / Tab"),
    ]),
    ("Editing", &[
        row("Undo / redo", "", "Ctrl+Z / Shift+Ctrl+Z"),
        row("Select all", "", "Ctrl+A"),
        row("Cut / copy / paste", "", "Ctrl+X / C / V"),
        row("Emoji", "", "Ctrl+."),
    ]),
    ("Rendered view", &[
        row("Follow a link", "", "Click"),
        row("Tick a checkbox", "", "Click"),
        row("Search for a #tag", "", "Click"),
        row("Fold or unfold a callout", "", "Click its title"),
        row("Edit the source", "", "Double-click"),
    ]),
];

/// Builds the sheet: a dimmed backdrop with a centered card. Clicking it closes it.
pub fn build(app: &gtk::Application, close: impl Fn() + 'static) -> gtk::Widget {
    let title = gtk::Label::builder().label("Keyboard shortcuts").css_classes(["nevis-help-title"]).build();
    let columns = gtk::Box::new(gtk::Orientation::Horizontal, 32);
    columns.append(&column(app, LEFT));
    columns.append(&column(app, RIGHT));
    let hint = gtk::Label::builder().label("Esc or click to close").css_classes(["dim-label"]).build();

    let card = gtk::Box::new(gtk::Orientation::Vertical, 16);
    card.add_css_class("nevis-help");
    card.append(&title);
    card.append(&columns);
    card.append(&hint);

    // Scrolls when the window is smaller than the card.
    let scroller = gtk::ScrolledWindow::builder()
        .child(&card)
        .propagate_natural_width(true)
        .propagate_natural_height(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .margin_top(16)
        .margin_bottom(16)
        .build();

    let backdrop = gtk::Box::new(gtk::Orientation::Vertical, 0);
    backdrop.add_css_class("nevis-help-backdrop");
    backdrop.set_hexpand(true);
    backdrop.set_vexpand(true);
    backdrop.append(&scroller);
    let click = gtk::GestureClick::new();
    click.connect_released(move |_, _, _, _| close());
    backdrop.add_controller(click);
    backdrop.set_visible(false);
    backdrop.upcast()
}

fn column(app: &gtk::Application, sections: &[(&str, &[Row])]) -> gtk::Grid {
    let grid = gtk::Grid::builder().row_spacing(6).column_spacing(24).valign(gtk::Align::Start).build();
    let mut line = 0;
    for (name, rows) in sections {
        let header = gtk::Label::builder().label(*name).xalign(0.0).css_classes(["nevis-help-section"]).build();
        if line > 0 {
            header.set_margin_top(12);
        }
        grid.attach(&header, 0, line, 2, 1);
        line += 1;
        for row in *rows {
            let what = gtk::Label::builder().label(row.what).xalign(0.0).build();
            let keys = gtk::Label::builder().label(keys(app, row)).xalign(0.0).css_classes(["nevis-help-keys"]).build();
            grid.attach(&what, 0, line, 1, 1);
            grid.attach(&keys, 1, line, 1, 1);
            line += 1;
        }
    }
    grid
}

/// "F3 / Enter": the action's main binding, then the keys handled elsewhere.
fn keys(app: &gtk::Application, row: &Row) -> String {
    let mut labels: Vec<String> = if row.action.is_empty() {
        Vec::new()
    } else {
        app.accels_for_action(row.action)
            .iter()
            .filter(|accel| !accel.contains("KP_"))
            .filter_map(|accel| gtk::accelerator_parse(accel.as_str()))
            .map(|(key, mods)| gtk::accelerator_get_label(key, mods).to_string())
            .take(1)
            .collect()
    };
    if !row.also.is_empty() {
        labels.push(row.also.to_owned());
    }
    labels.join(" / ")
}

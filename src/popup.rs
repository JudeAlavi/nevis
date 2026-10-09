//! A card over the window on a dimmed backdrop: the quick switcher, the folder search and the
//! name questions. Esc or a click outside the card hides it.

use gtk::{gdk, glib, prelude::*};

/// The backdrop (add it as an overlay; it starts hidden) and the card `width` wide on it.
pub fn card(width: i32) -> (gtk::Box, gtk::Box) {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("nevis-switcher");
    card.set_halign(gtk::Align::Center);
    card.set_valign(gtk::Align::Start);
    card.set_margin_top(48);
    card.set_width_request(width);

    let backdrop = gtk::Box::new(gtk::Orientation::Vertical, 0);
    backdrop.add_css_class("nevis-help-backdrop");
    backdrop.set_hexpand(true);
    backdrop.set_vexpand(true);
    backdrop.append(&card);
    backdrop.set_visible(false);

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(|keys, key, _, _| {
        if key == gdk::Key::Escape
            && let Some(backdrop) = keys.widget()
        {
            backdrop.set_visible(false);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    backdrop.add_controller(keys);
    let click = gtk::GestureClick::new();
    click.connect_released(|click, _, x, y| {
        let Some(backdrop) = click.widget() else { return };
        if backdrop.pick(x, y, gtk::PickFlags::DEFAULT).as_ref() == Some(&backdrop) {
            click.set_state(gtk::EventSequenceState::Claimed);
            backdrop.set_visible(false);
        }
    });
    backdrop.add_controller(click);
    (backdrop, card)
}

/// Up and Down move through `list`'s rows (round and round) while typing stays in `entry`.
pub fn browse_with_arrows(backdrop: &gtk::Box, list: &gtk::ListBox, entry: &gtk::Entry) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (list, entry) = (list.downgrade(), entry.downgrade());
    keys.connect_key_pressed(move |_, key, _, _| {
        let by = match key {
            gdk::Key::Down | gdk::Key::KP_Down => 1,
            gdk::Key::Up | gdk::Key::KP_Up => -1,
            _ => return glib::Propagation::Proceed,
        };
        let (Some(list), Some(entry)) = (list.upgrade(), entry.upgrade()) else { return glib::Propagation::Proceed };
        let count = list.observe_children().n_items() as i32;
        let current = list.selected_row().map_or(0, |row| row.index());
        if count > 0
            && let Some(row) = list.row_at_index((current + by).rem_euclid(count))
        {
            list.select_row(Some(&row));
            row.grab_focus();
            entry.grab_focus_without_selecting();
        }
        glib::Propagation::Stop
    });
    backdrop.add_controller(keys);
}

//! Find & replace bar (Ctrl+F / Ctrl+H), backed by GtkSourceView's search engine.
//!
//! Built on first use and drawn without icons: GTK's first icon lookup costs ~300 ms here,
//! which would otherwise land on startup or on the first Ctrl+F.

use std::rc::Rc;

use gtk::{gdk, glib, prelude::*};
use sourceview::prelude::*;

pub struct FindBar {
    pub widget: gtk::Box,
    entry: gtk::Entry,
    replace_entry: gtk::Entry,
    replace_row: gtk::Box,
    count: gtk::Label,
    settings: sourceview::SearchSettings,
    context: sourceview::SearchContext,
    view: sourceview::View,
    buffer: sourceview::Buffer,
}

impl FindBar {
    pub fn new(view: &sourceview::View, buffer: &sourceview::Buffer) -> Rc<Self> {
        let settings = sourceview::SearchSettings::new();
        settings.set_wrap_around(true);
        let context = sourceview::SearchContext::new(buffer, Some(&settings));

        let entry = gtk::Entry::builder().placeholder_text("Find").hexpand(true).build();
        let prev = button("↑", "Previous match (Shift+Enter, Shift+F3)");
        let next = button("↓", "Next match (Enter, F3)");
        let count = gtk::Label::builder().width_chars(10).css_classes(["dim-label"]).build();
        let case = gtk::ToggleButton::builder().label("Aa").tooltip_text("Match case").css_classes(["flat"]).build();
        let underlined = gtk::Label::builder().use_markup(true).label("<u>ab</u>").build();
        let word = gtk::ToggleButton::builder().child(&underlined).tooltip_text("Whole words").css_classes(["flat"]).build();
        let regex = gtk::ToggleButton::builder().label(".*").tooltip_text("Regular expression").css_classes(["flat"]).build();
        let close = button("✕", "Close (Esc)");
        let find_row = row(&[entry.upcast_ref(), prev.upcast_ref(), next.upcast_ref(), count.upcast_ref(),
                             case.upcast_ref(), word.upcast_ref(), regex.upcast_ref(), close.upcast_ref()]);

        let replace_entry = gtk::Entry::builder().placeholder_text("Replace with").hexpand(true).build();
        let replace_one = gtk::Button::with_label("Replace");
        let replace_all = gtk::Button::with_label("Replace All");
        let replace_row = row(&[replace_entry.upcast_ref(), replace_one.upcast_ref(), replace_all.upcast_ref()]);

        let widget = gtk::Box::new(gtk::Orientation::Vertical, 4);
        widget.add_css_class("nevis-find");
        widget.append(&find_row);
        widget.append(&replace_row);
        widget.set_visible(false);

        let bar = Rc::new(Self {
            widget,
            entry,
            replace_entry,
            replace_row,
            count,
            settings,
            context,
            view: view.clone(),
            buffer: buffer.clone(),
        });

        // Every handler holds the bar weakly; `with` wraps a method into such a handler.
        let weak = Rc::downgrade(&bar);
        let with = move |method: fn(&FindBar)| {
            let weak = weak.clone();
            move || {
                if let Some(bar) = weak.upgrade() {
                    method(&bar);
                }
            }
        };

        let on_change = with(Self::search_changed);
        bar.entry.connect_changed(move |_| on_change());
        let on_enter = with(Self::next);
        bar.entry.connect_activate(move |_| on_enter());
        let on_next = with(Self::next);
        next.connect_clicked(move |_| on_next());
        let on_prev = with(Self::prev);
        prev.connect_clicked(move |_| on_prev());
        let on_close = with(Self::hide);
        close.connect_clicked(move |_| on_close());
        let on_replace = with(Self::replace_current);
        bar.replace_entry.connect_activate(move |_| on_replace());
        let on_replace = with(Self::replace_current);
        replace_one.connect_clicked(move |_| on_replace());
        let on_all = with(Self::replace_all);
        replace_all.connect_clicked(move |_| on_all());

        let settings = bar.settings.clone();
        let on_case = with(Self::search_changed);
        case.connect_toggled(move |t| {
            settings.set_case_sensitive(t.is_active());
            on_case();
        });
        let settings = bar.settings.clone();
        let on_word = with(Self::search_changed);
        word.connect_toggled(move |t| {
            settings.set_at_word_boundaries(t.is_active());
            on_word();
        });
        let settings = bar.settings.clone();
        let on_regex = with(Self::search_changed);
        regex.connect_toggled(move |t| {
            settings.set_regex_enabled(t.is_active());
            on_regex();
        });

        let on_count = with(Self::update_count);
        bar.context.connect_occurrences_count_notify(move |_| on_count());
        let on_regex_error = with(Self::show_regex_error);
        bar.context.connect_regex_error_notify(move |_| on_regex_error());

        // Esc anywhere in the bar closes it; Shift+Enter in the find field goes back.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let on_escape = with(Self::hide);
        let on_prev = with(Self::prev);
        let entry = bar.entry.clone();
        keys.connect_key_pressed(move |_, key, _, state| match key {
            gdk::Key::Escape => {
                on_escape();
                glib::Propagation::Stop
            }
            gdk::Key::Return | gdk::Key::KP_Enter
                if state.contains(gdk::ModifierType::SHIFT_MASK)
                    && entry.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN) =>
            {
                on_prev();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        bar.widget.add_controller(keys);

        bar
    }

    /// Opens the bar, prefilled with the selected text when it is a single line.
    pub fn show(&self, with_replace: bool) {
        if let Some((start, end)) = self.buffer.selection_bounds() {
            let text = self.buffer.text(&start, &end, false);
            if !text.is_empty() && !text.contains('\n') {
                self.entry.set_text(&text);
            }
        }
        self.replace_row.set_visible(with_replace);
        self.widget.set_visible(true);
        self.context.set_highlight(true);
        self.entry.grab_focus();
    }

    pub fn hide(&self) {
        self.widget.set_visible(false);
        self.context.set_highlight(false);
        self.view.grab_focus();
    }

    pub fn next(&self) {
        if self.settings.search_text().is_none() {
            return self.show(false);
        }
        let (_, from) = self.selection();
        if let Some((start, end, _)) = self.context.forward(&from) {
            self.select(&start, &end);
        }
    }

    pub fn prev(&self) {
        if self.settings.search_text().is_none() {
            return self.show(false);
        }
        let (from, _) = self.selection();
        if let Some((start, end, _)) = self.context.backward(&from) {
            self.select(&start, &end);
        }
    }

    fn search_changed(&self) {
        let text = self.entry.text();
        self.settings.set_search_text((!text.is_empty()).then_some(text.as_str()));
        // Search as you type, starting from the current match or the cursor.
        let (from, _) = self.selection();
        match self.context.forward(&from) {
            Some((start, end, _)) => self.select(&start, &end),
            None => self.update_count(),
        }
    }

    fn replace_current(&self) {
        let (mut start, mut end) = self.selection();
        if self.context.occurrence_position(&start, &end) > 0 {
            let _ = self.context.replace(&mut start, &mut end, &self.replace_entry.text());
        }
        self.next();
    }

    fn replace_all(&self) {
        let _ = self.context.replace_all(&self.replace_entry.text());
        self.update_count();
    }

    fn selection(&self) -> (gtk::TextIter, gtk::TextIter) {
        self.buffer.selection_bounds().unwrap_or_else(|| {
            let cursor = self.buffer.iter_at_mark(&self.buffer.get_insert());
            (cursor, cursor)
        })
    }

    fn select(&self, start: &gtk::TextIter, end: &gtk::TextIter) {
        self.buffer.select_range(start, end);
        self.view.scroll_to_mark(&self.buffer.get_insert(), 0.2, false, 0.0, 0.0);
        self.update_count();
    }

    fn update_count(&self) {
        let total = self.context.occurrences_count();
        let text = if self.entry.text().is_empty() || total < 0 {
            String::new()
        } else if total == 0 {
            "No results".to_owned()
        } else {
            let (start, end) = self.selection();
            match self.context.occurrence_position(&start, &end) {
                pos if pos > 0 => format!("{pos} of {total}"),
                _ => format!("{total} found"),
            }
        };
        self.count.set_text(&text);
    }

    fn show_regex_error(&self) {
        match self.context.regex_error() {
            Some(err) => {
                self.entry.add_css_class("error");
                self.entry.set_tooltip_text(Some(err.message()));
            }
            None => {
                self.entry.remove_css_class("error");
                self.entry.set_tooltip_text(None);
            }
        }
    }
}

fn button(label: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder().label(label).tooltip_text(tooltip).css_classes(["flat"]).build()
}

fn row(children: &[&gtk::Widget]) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    for child in children {
        row.append(*child);
    }
    row
}

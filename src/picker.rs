//! The theme picker (Settings → Theme): System and the built-in palettes, each with a strip of its
//! colours. Moving through the list shows each one at once; Enter keeps it, Esc or a click
//! outside goes back to the one before.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::{glib, prelude::*};

use crate::color::Rgb;
use crate::palettes::Palette;
use crate::{popup, switcher, theme};

pub struct Picker {
    pub widget: gtk::Box,
    entry: gtk::Entry,
    list: gtk::ListBox,
    /// Every palette, and which of them the list shows, in order.
    choices: RefCell<Vec<Palette>>,
    shown: RefCell<Vec<usize>>,
    /// The palette in use when the picker opened, to go back to.
    before: RefCell<String>,
    /// Enter was pressed: the palette shown stays.
    kept: Cell<bool>,
}

impl Picker {
    pub fn new() -> Rc<Self> {
        let entry = gtk::Entry::builder().placeholder_text("Theme").build();
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).build();
        list.add_css_class("nevis-switcher-list");
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(460)
            .child(&list)
            .build();
        let (widget, card) = popup::card(440);
        card.append(&entry);
        card.append(&scroller);
        popup::browse_with_arrows(&widget, &list, &entry);

        let picker = Rc::new(Self {
            widget,
            entry,
            list,
            choices: RefCell::new(Vec::new()),
            shown: RefCell::new(Vec::new()),
            before: RefCell::new(theme::choice()),
            kept: Cell::new(false),
        });
        let weak = Rc::downgrade(&picker);
        picker.entry.connect_changed(move |_| {
            if let Some(picker) = weak.upgrade() {
                picker.fill();
            }
        });
        // The palette under the selection shows at once.
        let weak = Rc::downgrade(&picker);
        picker.list.connect_row_selected(move |_, row| {
            if let (Some(picker), Some(row)) = (weak.upgrade(), row) {
                picker.preview(row.index());
            }
        });
        let weak = Rc::downgrade(&picker);
        picker.entry.connect_activate(move |_| {
            if let Some(picker) = weak.upgrade() {
                picker.keep();
            }
        });
        let weak = Rc::downgrade(&picker);
        picker.list.connect_row_activated(move |_, _| {
            if let Some(picker) = weak.upgrade() {
                picker.keep();
            }
        });
        // Closed without Enter: back to the palette from before.
        let weak = Rc::downgrade(&picker);
        picker.widget.connect_visible_notify(move |widget| {
            if let Some(picker) = weak.upgrade()
                && !widget.is_visible()
                && !picker.kept.get()
            {
                let before = picker.before.borrow().clone();
                if theme::choice() != before {
                    theme::choose(&before, false);
                }
            }
        });
        picker
    }

    pub fn is_open(&self) -> bool {
        self.widget.is_visible()
    }

    pub fn show(&self) {
        *self.before.borrow_mut() = theme::choice();
        self.kept.set(false);
        *self.choices.borrow_mut() = theme::choices();
        self.entry.set_text("");
        self.fill();
        self.widget.set_visible(true);
        self.entry.grab_focus();
    }

    pub fn hide(&self) {
        self.widget.set_visible(false);
    }

    /// Lists the palettes whose names match what is typed; the one in use is selected.
    fn fill(&self) {
        let choices = self.choices.borrow();
        let names: Vec<String> = choices.iter().map(|palette| palette.name.clone()).collect();
        let shown = switcher::rank(&self.entry.text(), &names);
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        for &index in &shown {
            let label = gtk::Label::builder().use_markup(true).label(row_markup(&choices[index])).xalign(0.0).build();
            self.list.append(&label);
        }
        let current = theme::choice();
        let selected = if self.entry.text().is_empty() { shown.iter().position(|&i| choices[i].id == current).unwrap_or(0) } else { 0 };
        *self.shown.borrow_mut() = shown;
        drop(choices);
        if let Some(row) = self.list.row_at_index(selected as i32) {
            self.list.select_row(Some(&row));
            // Bring it into view once the list is laid out.
            glib::idle_add_local_once(move || {
                row.grab_focus();
            });
            let entry = self.entry.clone();
            glib::idle_add_local_once(move || {
                entry.grab_focus_without_selecting();
            });
        }
    }

    fn preview(&self, row: i32) {
        let id = self.shown.borrow().get(row as usize).map(|&i| self.choices.borrow()[i].id.clone());
        if let Some(id) = id
            && id != theme::choice()
        {
            theme::choose(&id, false);
        }
    }

    fn keep(&self) {
        if let Some(row) = self.list.selected_row() {
            let id = self.shown.borrow().get(row.index() as usize).map(|&i| self.choices.borrow()[i].id.clone());
            if let Some(id) = id {
                theme::choose(&id, true);
            }
        }
        self.kept.set(true);
        self.hide();
    }
}

/// "Gruvbox Dark  ● ● ● ● ● ●": the name, then the palette's background, accent and colours.
fn row_markup(palette: &Palette) -> String {
    let dot = |color: Rgb| format!("<span foreground=\"{}\">●</span>", color.to_hex());
    let colors = [palette.background, palette.foreground, palette.accent, palette.red, palette.green, palette.blue, palette.purple];
    let strip: Vec<String> = colors.into_iter().map(dot).collect();
    format!("{}   <span size=\"small\">{}</span>", glib::markup_escape_text(&palette.name), strip.join(" "))
}

//! The Settings page (Ctrl+,): every preference in one place, on a sheet over the window.
//! Each change applies at once; Esc or a click outside closes it.
//!
//! The controls are made of text and stylesheet only (a switch is a toggle button with a
//! knob), since GTK's own switch and drop-down load icons, which costs ~0.3 s the first time.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::{glib, prelude::*};

use crate::color::Rgb;
use crate::settings::{self, DEFAULT_TEXT_SIZE, Settings};
use crate::{popup, theme};

/// Brings a control back in line with the settings (shortcuts change them too).
type Refresh = Box<dyn Fn(&Settings)>;

pub struct SettingsPage {
    pub widget: gtk::Box,
    first: gtk::Widget,
    theme: gtk::Label,
    refreshers: RefCell<Vec<Refresh>>,
}

impl SettingsPage {
    /// `pick_theme` opens the theme picker (the page closes first).
    pub fn new(pick_theme: impl Fn() + 'static) -> Rc<Self> {
        let (widget, card) = popup::card(540);
        card.add_css_class("nevis-settings");
        card.set_margin_bottom(24);
        let body = gtk::Box::builder().orientation(gtk::Orientation::Vertical).margin_end(28).build();
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(620)
            .child(&body)
            .build();
        card.append(&gtk::Label::builder().label("Settings").xalign(0.0).margin_end(28).css_classes(["nevis-settings-title"]).build());
        card.append(&scroller);

        let theme = gtk::Label::builder().use_markup(true).xalign(1.0).build();
        let change = gtk::Button::builder().label("Change…").css_classes(["nevis-settings-button"]).build();
        let page = Rc::new(Self { widget, first: change.clone().upcast(), theme, refreshers: RefCell::new(Vec::new()) });

        section(&body, "Appearance");
        let theme_value = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        theme_value.append(&page.theme);
        theme_value.append(&change);
        let weak = Rc::downgrade(&page);
        change.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.hide();
            }
            pick_theme();
        });
        row(&body, "Theme", None, &theme_value);
        row(&body, "Text size", None, &page.text_size());
        let width = page.choice(&[("Full width", false), ("Centered", true)], |s| s.centered, |s, v| s.centered = v);
        row(&body, "Text width", Some("Centered keeps lines to a readable length, in both views"), &width);

        section(&body, "Writing");
        row(&body, "Wrap long lines", None, &page.switch(|s| s.wrap, |s, on| s.wrap = on));
        row(&body, "Line numbers", Some("In the source view"), &page.switch(|s| s.line_numbers, |s, on| s.line_numbers = on));
        let lists = page.switch(|s| s.continue_lists, |s, on| s.continue_lists = on);
        row(&body, "Continue lists on Enter", Some("Lists, checklists and quotes carry on to the next line"), &lists);
        let opens = page.choice(&[("Rendered", true), ("As source", false)], |s| s.open_rendered, |s, v| s.open_rendered = v);
        row(&body, "Markdown files open", None, &opens);
        let editing = page.choice(&[("Live preview", true), ("Source", false)], |s| s.live_preview, |s, v| s.live_preview = v);
        row(&body, "Edit Markdown as", Some("Live preview hides the markup except on the line you're editing"), &editing);
        let breaks = page.choice(&[("Shown", false), ("Joined", true)], |s| s.strict_line_breaks, |s, v| s.strict_line_breaks = v);
        row(&body, "Single line breaks", Some("Shown, as in Obsidian, or joined into the paragraph, as on GitHub"), &breaks);
        let spelling = page.switch(|s| s.spell_check, |s, on| s.spell_check = on);
        row(&body, "Check spelling", Some("Right-click a word for corrections and the language. Code and Persian are skipped."), &spelling);
        let autosave = page.switch(|s| s.autosave, |s, on| s.autosave = on);
        row(&body, "Save automatically", Some("A second after you stop typing. New notes still ask for a name first."), &autosave);

        section(&body, "Windows and tabs");
        let outside = page.choice(&[("A new window", true), ("A tab", false)], |s| s.outside_in_window, |s, v| s.outside_in_window = v);
        row(&body, "Files opened from outside go in", Some("From a file manager or a terminal"), &outside);
        let bar = page.choice(&[("Two tabs or more", false), ("Always", true)], |s| s.tab_bar_always, |s, v| s.tab_bar_always = v);
        row(&body, "Show the tab bar with", None, &bar);
        page
    }

    pub fn is_open(&self) -> bool {
        self.widget.is_visible()
    }

    pub fn show(&self) {
        self.refresh();
        self.widget.set_visible(true);
        self.first.grab_focus();
    }

    /// Shows the settings as they are now.
    pub fn refresh(&self) {
        let settings = settings::get();
        for refresh in self.refreshers.borrow().iter() {
            refresh(&settings);
        }
        self.show_theme();
    }

    pub fn hide(&self) {
        self.widget.set_visible(false);
    }

    /// The palette in use: its name, then a strip of its colours.
    fn show_theme(&self) {
        let choice = theme::choice();
        let Some(palette) = theme::choices().into_iter().find(|p| p.id == choice) else { return };
        let dot = |c: Rgb| format!("<span foreground=\"{}\">●</span>", c.to_hex());
        let dots: Vec<String> = [palette.accent, palette.red, palette.green, palette.blue, palette.purple].into_iter().map(dot).collect();
        self.theme.set_markup(&format!("{}  <span size=\"small\">{}</span>", glib::markup_escape_text(&palette.name), dots.join("")));
    }

    /// An on/off switch for the setting `get` reads and `set` writes.
    fn switch(&self, get: fn(&Settings) -> bool, set: fn(&mut Settings, bool)) -> gtk::ToggleButton {
        let knob = gtk::Box::builder().css_classes(["knob"]).halign(gtk::Align::Start).valign(gtk::Align::Center).build();
        let toggle = gtk::ToggleButton::builder()
            .child(&knob)
            .css_classes(["nevis-switch"])
            .halign(gtk::Align::End)
            .valign(gtk::Align::Center)
            .active(get(&settings::get()))
            .build();
        toggle.connect_toggled(move |toggle| settings::change(|s| set(s, toggle.is_active())));
        let shown = toggle.clone();
        self.refreshers.borrow_mut().push(Box::new(move |s| shown.set_active(get(s))));
        toggle
    }

    /// One of a few `options` (label, value) for the setting `get` reads and `set` writes.
    fn choice<T: Copy + PartialEq + 'static>(&self, options: &[(&str, T)], get: fn(&Settings) -> T, set: fn(&mut Settings, T)) -> gtk::Box {
        let group = gtk::Box::builder().css_classes(["nevis-segments"]).halign(gtk::Align::End).valign(gtk::Align::Center).build();
        let mut buttons: Vec<(gtk::ToggleButton, T)> = Vec::new();
        for &(label, value) in options {
            let button = gtk::ToggleButton::builder().label(label).active(get(&settings::get()) == value).build();
            if let Some((first, _)) = buttons.first() {
                button.set_group(Some(first));
            }
            button.connect_toggled(move |button| {
                if button.is_active() {
                    settings::change(|s| set(s, value));
                }
            });
            group.append(&button);
            buttons.push((button, value));
        }
        self.refreshers.borrow_mut().push(Box::new(move |s| {
            for (button, value) in &buttons {
                if get(s) == *value {
                    button.set_active(true);
                }
            }
        }));
        group
    }

    /// "−  12 pt  +", and a way back to the default size.
    fn text_size(&self) -> gtk::Box {
        let minus = gtk::Button::builder().label("−").tooltip_text("Smaller (Ctrl+−)").build();
        let plus = gtk::Button::builder().label("+").tooltip_text("Larger (Ctrl++)").build();
        let value = gtk::Label::builder().width_chars(6).css_classes(["nevis-stepper-value"]).build();
        minus.connect_clicked(|_| settings::change(|s| s.text_size -= 1.0));
        plus.connect_clicked(|_| settings::change(|s| s.text_size += 1.0));
        let stepper = gtk::Box::builder().css_classes(["nevis-segments", "nevis-stepper"]).valign(gtk::Align::Center).build();
        stepper.append(&minus);
        stepper.append(&value);
        stepper.append(&plus);
        let reset = gtk::Button::builder().label("Reset").css_classes(["nevis-settings-button"]).valign(gtk::Align::Center).build();
        reset.connect_clicked(|_| settings::change(|s| s.text_size = DEFAULT_TEXT_SIZE));
        let both = gtk::Box::builder().spacing(10).halign(gtk::Align::End).build();
        both.append(&reset);
        both.append(&stepper);
        let (shown, again) = (value.clone(), reset.clone());
        self.refreshers.borrow_mut().push(Box::new(move |s| {
            shown.set_text(&format!("{} pt", s.text_size));
            again.set_visible(s.text_size != DEFAULT_TEXT_SIZE);
        }));
        both
    }
}

fn section(body: &gtk::Box, title: &str) {
    body.append(&gtk::Label::builder().label(title).xalign(0.0).css_classes(["nevis-settings-section"]).build());
}

/// A setting: its name (and a line about it) on the left, its control on the right.
fn row(body: &gtk::Box, name: &str, about: Option<&str>, control: &impl IsA<gtk::Widget>) {
    let words = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).hexpand(true).valign(gtk::Align::Center).build();
    words.append(&gtk::Label::builder().label(name).xalign(0.0).wrap(true).build());
    if let Some(about) = about {
        words.append(&gtk::Label::builder().label(about).xalign(0.0).wrap(true).css_classes(["nevis-settings-about"]).build());
    }
    let line = gtk::Box::builder().spacing(24).css_classes(["nevis-settings-row"]).build();
    line.append(&words);
    line.append(control);
    control.upcast_ref::<gtk::Widget>().update_property(&[gtk::accessible::Property::Label(name)]);
    body.append(&line);
}

//! Completion while typing a note: `[[` offers the vault's notes (and today's, tomorrow's,
//! yesterday's), `[[Note#` its headings, `#` the vault's tags, `> [!` the callout types. Up and
//! Down choose, Enter or Tab puts it in, Esc closes. Drawn in the tab, under the cursor.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::{glib, prelude::*};

/// What is being typed where the cursor is.
#[derive(Clone, Debug, PartialEq)]
pub enum Context {
    /// A link's note: `[[pla`.
    Link(String),
    /// A heading of the note before the `#`: `[[Plans#bud` (an empty note: this one).
    Heading(String, String),
    /// A tag: `#proj`.
    Tag(String),
    /// A callout type: `> [!war`.
    Callout(String),
}

/// What is being typed at the end of `before` (the line up to the cursor), and how many
/// characters back it starts: those a choice replaces.
pub fn context(before: &str) -> Option<(Context, usize)> {
    if let Some(open) = before.rfind("[[") {
        let inside = &before[open + 2..];
        if !inside.contains("]]") && !inside.contains('|') {
            return Some(match inside.split_once('#') {
                Some((note, heading)) => (Context::Heading(note.to_owned(), heading.to_owned()), heading.chars().count()),
                None => (Context::Link(inside.to_owned()), inside.chars().count()),
            });
        }
    }
    let quoted = before.trim_start();
    if quoted.starts_with('>') {
        let marker = quoted.trim_start_matches(['>', ' ']);
        if let Some(kind) = marker.strip_prefix("[!")
            && !kind.contains(']')
            && kind.chars().all(|c| c.is_alphanumeric() || c == '-')
        {
            return Some((Context::Callout(kind.to_owned()), kind.chars().count()));
        }
    }
    let hash = before.rfind('#')?;
    let word = &before[hash + 1..];
    let after_space = before[..hash].chars().last().is_none_or(char::is_whitespace);
    let tag_like = word.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/' | '\u{200C}'));
    (after_space && !word.is_empty() && tag_like).then(|| (Context::Tag(word.to_owned()), word.chars().count()))
}

/// One choice: what is shown (Pango markup) and what it puts in.
pub struct Offer {
    pub markup: String,
    pub text: String,
}

/// The list of choices, drawn over the tab.
pub struct Completion {
    pub widget: gtk::Box,
    list: gtk::ListBox,
    offers: RefCell<Vec<Offer>>,
}

/// Most rows shown.
pub const ROWS: usize = 10;

impl Completion {
    pub fn new() -> Rc<Self> {
        // Never focused: the typing stays in the text.
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).focusable(false).build();
        list.add_css_class("nevis-complete-list");
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .width_request(320)
            .css_classes(["nevis-complete"])
            .visible(false)
            .build();
        widget.append(&list);
        Rc::new(Self { widget, list, offers: RefCell::new(Vec::new()) })
    }

    pub fn is_open(&self) -> bool {
        self.widget.is_visible()
    }

    /// Shows `offers` with the card's corner at `x`, `y` (in the tab), the first one chosen.
    pub fn show(&self, offers: Vec<Offer>, x: i32, y: i32) {
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        for offer in &offers {
            let label = gtk::Label::builder()
                .use_markup(true)
                .label(&offer.markup)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build();
            self.list.append(&label);
            if let Some(row) = self.list.last_child().and_downcast::<gtk::ListBoxRow>() {
                row.set_focusable(false);
            }
        }
        *self.offers.borrow_mut() = offers;
        self.list.select_row(self.list.row_at_index(0).as_ref());
        self.widget.set_margin_start(x.max(0));
        self.widget.set_margin_top(y.max(0));
        self.widget.set_visible(true);
    }

    pub fn hide(&self) {
        self.widget.set_visible(false);
    }

    /// Chooses the row `by` rows down (up when negative), wrapping around.
    pub fn step(&self, by: i32) {
        let count = self.offers.borrow().len() as i32;
        if count == 0 {
            return;
        }
        let at = self.list.selected_row().map_or(0, |row| row.index());
        let next = (at + by).rem_euclid(count);
        if let Some(row) = self.list.row_at_index(next) {
            self.list.select_row(Some(&row));
        }
    }

    /// Runs `f` when a row is clicked (after choosing it).
    pub fn on_click(&self, f: impl Fn() + 'static) {
        self.list.connect_row_activated(move |list, row| {
            list.select_row(Some(row));
            f();
        });
    }

    /// What the chosen row puts in.
    pub fn chosen(&self) -> Option<String> {
        let at = self.list.selected_row()?.index() as usize;
        self.offers.borrow().get(at).map(|offer| offer.text.clone())
    }
}

/// "Name  folder", the folder dimmed.
pub fn markup(name: &str, hint: &str) -> String {
    let name = glib::markup_escape_text(name);
    if hint.is_empty() { name.to_string() } else { format!("{name}  <span alpha=\"55%\">{}</span>", glib::markup_escape_text(hint)) }
}

/// The callout types Obsidian knows, and their other names.
pub const CALLOUTS: [&str; 27] = [
    "note", "abstract", "summary", "tldr", "info", "todo", "tip", "hint", "important", "success", "check", "done",
    "question", "help", "faq", "warning", "caution", "attention", "failure", "fail", "missing", "danger", "error",
    "bug", "example", "quote", "cite",
];

//! The outline (Ctrl+R): the note's headings, to jump to one. Typing narrows the list;
//! Enter or a click goes there, Esc or a click outside closes it.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::{glib, prelude::*};
use pulldown_cmark::{Event, Tag, TagEnd};

use crate::{popup, render, switcher};

/// Called with the headings shown and the index of the one picked.
type Jump = Box<dyn Fn(usize, &[Heading])>;

pub struct Outline {
    pub widget: gtk::Box,
    entry: gtk::Entry,
    list: gtk::ListBox,
    headings: RefCell<Vec<Heading>>,
    /// Which headings the list shows, in order.
    shown: RefCell<Vec<usize>>,
    jump: Jump,
}

impl Outline {
    pub fn new(jump: impl Fn(usize, &[Heading]) + 'static) -> Rc<Self> {
        let entry = gtk::Entry::builder().placeholder_text("Go to heading").build();
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).build();
        list.add_css_class("nevis-switcher-list");
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(460)
            .child(&list)
            .build();
        let (widget, card) = popup::card(480);
        card.append(&entry);
        card.append(&scroller);
        popup::browse_with_arrows(&widget, &list, &entry);
        let outline = Rc::new(Self {
            widget,
            entry,
            list,
            headings: RefCell::new(Vec::new()),
            shown: RefCell::new(Vec::new()),
            jump: Box::new(jump),
        });
        let weak = Rc::downgrade(&outline);
        outline.entry.connect_changed(move |_| {
            if let Some(outline) = weak.upgrade() {
                outline.fill(None);
            }
        });
        let weak = Rc::downgrade(&outline);
        outline.entry.connect_activate(move |_| {
            if let Some(outline) = weak.upgrade() {
                outline.go();
            }
        });
        let weak = Rc::downgrade(&outline);
        outline.list.connect_row_activated(move |_, _| {
            if let Some(outline) = weak.upgrade() {
                outline.go();
            }
        });
        outline
    }

    pub fn is_open(&self) -> bool {
        self.widget.is_visible()
    }

    /// Opens on `headings`, with `current` (the heading the reader is in) selected.
    pub fn show(&self, headings: Vec<Heading>, current: Option<usize>) {
        *self.headings.borrow_mut() = headings;
        self.entry.set_text("");
        self.fill(current);
        self.widget.set_visible(true);
        self.entry.grab_focus();
    }

    pub fn hide(&self) {
        self.widget.set_visible(false);
    }

    /// Lists the headings matching what is typed, indented by level; selects `current`, or
    /// the best match.
    fn fill(&self, current: Option<usize>) {
        let headings = self.headings.borrow();
        let texts: Vec<String> = headings.iter().map(|heading| heading.text.clone()).collect();
        let shown = switcher::rank(&self.entry.text(), &texts);
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        if headings.is_empty() {
            let empty = gtk::Label::builder()
                .label("No headings here. Lines starting with # are headings.")
                .xalign(0.0)
                .css_classes(["nevis-outline-empty"])
                .build();
            self.list.append(&empty);
            if let Some(row) = self.list.row_at_index(0) {
                row.set_activatable(false);
                row.set_selectable(false);
            }
        }
        // The note's top level sits at the left edge, whichever level it is.
        let top = headings.iter().map(|heading| heading.level).min().unwrap_or(1);
        for &index in &shown {
            let heading = &headings[index];
            let label = gtk::Label::builder()
                .label(&heading.text)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .margin_start(16 * i32::from(heading.level - top))
                .build();
            if heading.level == top {
                label.add_css_class("nevis-outline-top");
            }
            self.list.append(&label);
        }
        let selected = current.and_then(|current| shown.iter().position(|&i| i == current)).unwrap_or(0);
        *self.shown.borrow_mut() = shown;
        drop(headings);
        if let Some(row) = self.list.row_at_index(selected as i32).filter(|row| row.is_selectable()) {
            self.list.select_row(Some(&row));
            // Bring it into view once the list is laid out, then give the typing back.
            glib::idle_add_local_once(move || {
                row.grab_focus();
            });
            let entry = self.entry.clone();
            glib::idle_add_local_once(move || {
                entry.grab_focus_without_selecting();
            });
        }
    }

    /// Goes to the selected heading.
    fn go(&self) {
        let Some(row) = self.list.selected_row() else { return };
        let index = self.shown.borrow().get(row.index() as usize).copied();
        if let Some(index) = index {
            self.hide();
            (self.jump)(index, &self.headings.borrow());
        }
    }
}

/// A heading in a note: its level (1 for `#`), its text, and the line it starts on.
#[derive(Debug, PartialEq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub line: usize,
}

/// The note's headings, in order: the same ones, in the same order, as the rendered view shows.
pub fn headings(markdown: &str) -> Vec<Heading> {
    let mut found = Vec::new();
    let mut current: Option<Heading> = None;
    for (event, range) in render::events(markdown) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let line = markdown[..range.start].matches('\n').count();
                current = Some(Heading { level: level as u8, text: String::new(), line });
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(heading) = &mut current {
                    heading.text.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => found.extend(current.take()),
            _ => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heading(level: u8, text: &str, line: usize) -> Heading {
        Heading { level, text: text.to_owned(), line }
    }

    #[test]
    fn lists_headings_with_their_level_and_line() {
        let note = "# Plans\n\nSome text.\n\n## This week\n- one\n### Monday\n";
        assert_eq!(headings(note), vec![heading(1, "Plans", 0), heading(2, "This week", 4), heading(3, "Monday", 6)]);
    }

    #[test]
    fn a_rule_under_the_first_lines_does_not_hide_the_headings_after_it() {
        let note = "*2025-08-09 20:37*\n**tags** : [[algorithm]]\n\n---\n# Progress\n\n## u01\n- [x] one\n---\n";
        assert_eq!(headings(note), vec![heading(1, "Progress", 4), heading(2, "u01", 6)]);
    }

    #[test]
    fn code_is_not_a_heading_underlined_titles_are_and_markup_is_dropped() {
        let note = "Title\n=====\n\n```sh\n# a comment\n```\n\n## **Bold** `code` [link](x.md)\n\n### فهرست کارها\n";
        assert_eq!(
            headings(note),
            vec![heading(1, "Title", 0), heading(2, "Bold code link", 7), heading(3, "فهرست کارها", 9)]
        );
    }
}

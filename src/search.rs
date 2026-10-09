//! Searching the text of the notes in a folder (Ctrl+Shift+F).

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gtk::{gio, glib, pango, prelude::*};

use crate::switcher::{files_under, row_markup};
use crate::{devhooks, popup, theme};

/// A line where the query was found.
#[derive(Debug, PartialEq)]
pub struct Hit {
    /// The line, counting from 0.
    pub line: usize,
    /// That line, trimmed around the match.
    pub snippet: String,
    /// Where the match is in `snippet`, in bytes.
    pub matched: Range<usize>,
    /// Where the match is on the line, in bytes.
    pub in_line: Range<usize>,
}

/// Every line of `text` containing `query`, ignoring case.
pub fn hits(query: &str, text: &str) -> Vec<Hit> {
    let query = fold(query.trim(), None);
    // Most files don't have it: those are folded once, without keeping track of where.
    if query.is_empty() || !fold(text, None).contains(&query) {
        return Vec::new();
    }
    let mut map = Vec::with_capacity(text.len() + 1);
    let folded = fold(text, Some(&mut map));
    let mut found: Vec<Hit> = Vec::new();
    // Lines are counted in `text`, as far as `counted`.
    let (mut line, mut counted) = (0, 0);
    for (at, _) in folded.match_indices(&query) {
        let matched = map[at]..map[at + query.len()];
        line += text[counted..matched.start].matches('\n').count();
        counted = matched.start;
        if found.last().is_none_or(|hit| hit.line != line) {
            found.push(hit(text, line, matched));
        }
    }
    found
}

/// `text` the way it is compared; `map` gets, for each of its bytes (and its end), the byte
/// in `text` it came from.
fn fold(text: &str, mut map: Option<&mut Vec<usize>>) -> String {
    let mut folded = String::with_capacity(text.len());
    for (at, c) in text.char_indices() {
        let c = match c {
            c if c.is_ascii() => c.to_ascii_lowercase(),
            // Vowel marks and half-spaces don't change the word.
            '\u{64B}'..='\u{65F}' | '\u{670}' | '\u{200C}' => continue,
            // Arabic yeh and kaf, as an Arabic keyboard types them, are Persian ی and ک.
            '\u{64A}' | '\u{649}' => 'ی',
            '\u{643}' => 'ک',
            // Persian ۰–۹ and Arabic ٠–٩ are the digits 0–9.
            '۰'..='۹' => char::from(b'0' + (c as u32 - '۰' as u32) as u8),
            '٠'..='٩' => char::from(b'0' + (c as u32 - '٠' as u32) as u8),
            c => c,
        };
        // Hebrew and Arabic letters have no case; looking that up is most of the time spent.
        if c.is_ascii() || ('\u{590}'..='\u{6FF}').contains(&c) {
            folded.push(c);
        } else {
            folded.extend(c.to_lowercase());
        }
        if let Some(map) = map.as_deref_mut() {
            map.resize(folded.len(), at);
        }
    }
    if let Some(map) = map {
        map.push(text.len());
    }
    folded
}

/// How much of its line a hit shows: characters before the match, and in all.
const BEFORE: usize = 40;
const SHOWN: usize = 120;

/// The hit for `matched` (a range of `text`) on `line`.
fn hit(text: &str, line: usize, matched: Range<usize>) -> Hit {
    let line_start = text[..matched.start].rfind('\n').map_or(0, |newline| newline + 1);
    let line_end = text[matched.end..].find('\n').map_or(text.len(), |newline| matched.end + newline);
    // Indentation and line-end whitespace (a "\r" too) are left out.
    let mut start = matched.start - text[line_start..matched.start].trim_start().len();
    let mut end = matched.end + text[matched.end..line_end].trim_end().len();
    let before = text[start..matched.start].chars().count();
    let cut_front = before > BEFORE;
    if cut_front {
        start += text[start..].char_indices().nth(before - BEFORE).map_or(0, |(at, _)| at);
    }
    let cut_back = text[start..end].chars().count() > SHOWN;
    if cut_back {
        end = text[start..].char_indices().nth(SHOWN).map_or(end, |(at, _)| start + at).max(matched.end);
    }
    let mut snippet = String::from(if cut_front { "…" } else { "" });
    let offset = snippet.len();
    snippet.push_str(&text[start..end]);
    if cut_back {
        snippet.push('…');
    }
    let in_line = matched.start - line_start..matched.end - line_start;
    Hit { line, snippet, matched: matched.start - start + offset..matched.end - start + offset, in_line }
}

/// Most hits listed; the search stops there.
const MOST: usize = 200;
/// Files bigger than this aren't notes; they are skipped.
const BIGGEST: u64 = 4 << 20;

/// A hit, and the file it is in (relative to the folder searched).
type Found = (String, Hit);

/// The popup searching the text of every note under a folder.
pub struct Search {
    pub widget: gtk::Box,
    entry: gtk::Entry,
    status: gtk::Label,
    scroller: gtk::ScrolledWindow,
    list: gtk::ListBox,
    root: RefCell<Option<gio::File>>,
    /// The files under the root, listed by the first search after the popup opens.
    files: RefCell<Option<Arc<Vec<String>>>>,
    /// What the list shows, in order.
    found: RefCell<Vec<Found>>,
    /// Counts searches; one still running stops once a newer one starts.
    searches: Arc<AtomicUsize>,
    /// Which search the list shows.
    shown: Cell<usize>,
    /// Enter came before the results: open the first one when they are in.
    open_when_found: Cell<bool>,
    /// The search waiting for the typing to pause.
    pending: RefCell<Option<glib::SourceId>>,
    pick: Box<dyn Fn(gio::File, usize, Range<usize>)>,
}

impl Search {
    /// `pick` is called with the file, line and place on the line of the hit the user chose.
    pub fn new(pick: impl Fn(gio::File, usize, Range<usize>) + 'static) -> Rc<Self> {
        let entry = gtk::Entry::builder().placeholder_text("Search in folder").build();
        let status = gtk::Label::builder().xalign(0.0).css_classes(["nevis-search-status"]).visible(false).build();
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).build();
        list.add_css_class("nevis-switcher-list");
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(480)
            .child(&list)
            .visible(false)
            .build();
        let (widget, card) = popup::card(680);
        card.append(&entry);
        card.append(&status);
        card.append(&scroller);
        popup::browse_with_arrows(&widget, &list, &entry);

        let search = Rc::new(Self {
            widget,
            entry,
            status,
            scroller,
            list,
            root: RefCell::new(None),
            files: RefCell::new(None),
            found: RefCell::new(Vec::new()),
            searches: Arc::new(AtomicUsize::new(0)),
            shown: Cell::new(0),
            open_when_found: Cell::new(false),
            pending: RefCell::new(None),
            pick: Box::new(pick),
        });
        let weak = Rc::downgrade(&search);
        search.entry.connect_changed(move |_| {
            if let Some(search) = weak.upgrade() {
                search.schedule();
            }
        });
        let weak = Rc::downgrade(&search);
        search.entry.connect_activate(move |_| {
            if let Some(search) = weak.upgrade() {
                search.accept();
            }
        });
        let weak = Rc::downgrade(&search);
        search.list.connect_row_activated(move |list, row| {
            list.select_row(Some(row));
            if let Some(search) = weak.upgrade() {
                search.choose_selected();
            }
        });
        search
    }

    pub fn is_open(&self) -> bool {
        self.widget.is_visible()
    }

    /// Opens the search on the notes under `root`, searching again for what was typed last.
    pub fn show(self: &Rc<Self>, root: &gio::File) {
        if self.root.borrow().as_ref().is_none_or(|r| !r.equal(root)) {
            *self.root.borrow_mut() = Some(root.clone());
            self.fill(self.searches.load(Ordering::SeqCst), Vec::new(), false);
        }
        // Notes may have come and gone since the last time.
        self.files.borrow_mut().take();
        self.widget.set_visible(true);
        self.entry.grab_focus();
        self.run();
    }

    pub fn hide(&self) {
        self.widget.set_visible(false);
    }

    /// Searches once the typing pauses.
    fn schedule(self: &Rc<Self>) {
        if let Some(waiting) = self.pending.take() {
            waiting.remove();
        }
        let weak = Rc::downgrade(self);
        let waiting = glib::timeout_add_local_once(Duration::from_millis(120), move || {
            if let Some(search) = weak.upgrade() {
                search.pending.take();
                search.run();
            }
        });
        *self.pending.borrow_mut() = Some(waiting);
    }

    /// Searches the files in the background; the list fills in when that's done.
    fn run(self: &Rc<Self>) {
        let query = self.entry.text().to_string();
        let this = self.searches.fetch_add(1, Ordering::SeqCst) + 1;
        let root = self.root.borrow().as_ref().and_then(|root| root.path());
        let (Some(root), false) = (root, query.trim().is_empty()) else {
            self.fill(this, Vec::new(), false);
            return;
        };
        let (files, searches) = (self.files.borrow().clone(), self.searches.clone());
        devhooks::lap("search started");
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let searched = gio::spawn_blocking(move || {
                let files = files.unwrap_or_else(|| Arc::new(files_under(&root)));
                let stale = || searches.load(Ordering::SeqCst) != this;
                // A few threads, each through its share of the files; their finds stay in order.
                let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(4));
                let share = files.len().div_ceil(threads).max(1);
                let shares: Vec<Option<Vec<Found>>> = std::thread::scope(|scope| {
                    let running: Vec<_> =
                        files.chunks(share).map(|part| scope.spawn(|| search_files(&root, part, &query, &stale))).collect();
                    running.into_iter().map(|thread| thread.join().unwrap_or(None)).collect()
                });
                let mut found: Vec<Found> = shares.into_iter().collect::<Option<Vec<_>>>()?.into_iter().flatten().collect();
                let more = found.len() > MOST;
                found.truncate(MOST);
                Some((files, found, more))
            });
            let Ok(Some((files, found, more))) = searched.await else { return };
            let Some(search) = weak.upgrade() else { return };
            *search.files.borrow_mut() = Some(files);
            if search.searches.load(Ordering::SeqCst) == this {
                devhooks::lap("search done");
                search.fill(this, found, more);
            }
        });
    }

    /// Lists what search number `this` found; `more` when it stopped before the end.
    fn fill(&self, this: usize, found: Vec<Found>, more: bool) {
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        for (path, hit) in &found {
            let place = gtk::Label::builder()
                .use_markup(true)
                .label(format!("{}  <span alpha=\"55%\">· line {}</span>", row_markup(path), hit.line + 1))
                .xalign(0.0)
                .ellipsize(pango::EllipsizeMode::Middle)
                .css_classes(["nevis-search-place"])
                .build();
            // Right-to-left lines (Persian, Arabic, Hebrew) sit on the right.
            let rtl = pango::find_base_dir(&hit.snippet) == pango::Direction::Rtl;
            let snippet = gtk::Label::builder()
                .use_markup(true)
                .label(snippet_markup(hit))
                .xalign(if rtl { 1.0 } else { 0.0 })
                .ellipsize(pango::EllipsizeMode::End)
                .build();
            let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
            row.append(&place);
            row.append(&snippet);
            self.list.append(&row);
        }
        let typed = !self.entry.text().trim().is_empty();
        self.status.set_text(&match (found.len(), more) {
            (0, _) => "No matches".to_owned(),
            (count, true) => format!("The first {count} matching lines"),
            (1, false) => "1 matching line".to_owned(),
            (count, false) => format!("{count} matching lines"),
        });
        self.status.set_visible(typed);
        self.scroller.set_visible(!found.is_empty());
        self.list.select_row(self.list.row_at_index(0).as_ref());
        self.scroller.vadjustment().set_value(0.0);
        *self.found.borrow_mut() = found;
        self.shown.set(this);
        // Unless the popup was closed meanwhile.
        if self.open_when_found.replace(false) && self.is_open() {
            self.choose_selected();
        }
    }

    /// Enter in the field: opens the chosen hit, or the first one once the search is done.
    fn accept(self: &Rc<Self>) {
        if let Some(waiting) = self.pending.take() {
            waiting.remove();
            self.run();
        }
        if self.shown.get() == self.searches.load(Ordering::SeqCst) {
            self.choose_selected();
        } else {
            self.open_when_found.set(true);
        }
    }

    fn choose_selected(&self) {
        let Some(row) = self.list.selected_row() else { return };
        let chosen = self.found.borrow().get(row.index() as usize).map(|(path, hit)| (path.clone(), hit.line, hit.in_line.clone()));
        let root = self.root.borrow().clone();
        if let (Some((path, line, in_line)), Some(root)) = (chosen, root) {
            self.hide();
            (self.pick)(root.resolve_relative_path(path), line, in_line);
        }
    }
}

/// The hits in `files` (under `root`), stopping after one more than `MOST`, or as soon as the
/// search is `stale`.
fn search_files(root: &Path, files: &[String], query: &str, stale: &dyn Fn() -> bool) -> Option<Vec<Found>> {
    let mut found = Vec::new();
    for path in files {
        if stale() {
            return None;
        }
        let path_on_disk = root.join(path);
        if std::fs::metadata(&path_on_disk).is_ok_and(|m| m.len() > BIGGEST) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path_on_disk) else { continue };
        found.extend(hits(query, &text).into_iter().map(|hit| (path.clone(), hit)));
        if found.len() > MOST {
            break;
        }
    }
    Some(found)
}

/// The snippet, with the match marked.
fn snippet_markup(hit: &Hit) -> String {
    let (text, Range { start, end }) = (&hit.snippet, hit.matched.clone());
    let escape = |text: &str| glib::markup_escape_text(text).to_string();
    // A background only, so Persian letters still join across the edges of the match.
    format!(
        "{}<span background=\"{}\" bgalpha=\"30%\">{}</span>{}",
        escape(&text[..start]),
        theme::colors().accent.to_hex(),
        escape(&text[start..end]),
        escape(&text[end..]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_match_gives_its_line_and_the_text_around_it() {
        let found = hits("rust", "First line\nI like Rust a lot\nlast");
        let snippet = "I like Rust a lot".to_owned();
        assert_eq!(found, vec![Hit { line: 1, snippet, matched: 7..11, in_line: 7..11 }]);
    }

    /// The text each hit matched, as it is written in the note.
    fn matched(found: &[Hit]) -> Vec<&str> {
        found.iter().map(|hit| &hit.snippet[hit.matched.clone()]).collect::<Vec<_>>()
    }

    #[test]
    fn arabic_letters_vowel_marks_and_half_spaces_do_not_get_in_the_way() {
        // Arabic yeh (U+064A), alef maksura (U+0649) and kaf (U+0643) against Persian ی and ک.
        assert_eq!(matched(&hits("کتابی", "\u{64A}\u{643} \u{643}تاب\u{64A} خوب")), ["\u{643}تاب\u{64A}"]);
        assert_eq!(matched(&hits("\u{643}تاب", "این کتاب")), ["کتاب"]);
        assert_eq!(matched(&hits("علی", "عل\u{649}")), ["عل\u{649}"]);
        // A kasra (U+0650) in the text.
        assert_eq!(matched(&hits("کتاب", "ک\u{650}تاب")), ["ک\u{650}تاب"]);
        // A half-space (U+200C) in the text, or in the query.
        assert_eq!(matched(&hits("میروم", "من می\u{200C}روم")), ["می\u{200C}روم"]);
        assert_eq!(matched(&hits("می\u{200C}روم", "من میروم")), ["میروم"]);
    }

    #[test]
    fn persian_and_arabic_digits_are_the_same_digits() {
        assert_eq!(matched(&hits("1403", "سال ۱۴۰۳ بود")), ["۱۴۰۳"]);
        assert_eq!(matched(&hits("۲۵", "page 25")), ["25"]);
        // Arabic-Indic ٧ (U+0667).
        assert_eq!(matched(&hits("7", "\u{667}")), ["\u{667}"]);
    }

    #[test]
    fn long_lines_are_cut_around_the_match() {
        let found = hits("needle", &format!("{}needle{}", "a".repeat(100), "b".repeat(200)));
        let snippet = format!("…{}needle{}…", "a".repeat(40), "b".repeat(120 - 40 - 6));
        assert_eq!(found[0].snippet, snippet);
        assert_eq!(matched(&found), ["needle"]);
    }

    #[test]
    fn indentation_and_line_ends_are_left_out() {
        let found = hits("item", "    - an item  \r\nnext");
        assert_eq!(found[0].snippet, "- an item");
        assert_eq!(matched(&found), ["item"]);
        // Where the match is on the line itself, to select it there.
        assert_eq!(found[0].in_line, 9..13);
    }

    #[test]
    fn each_line_is_listed_once() {
        let found = hits("ab", "ab ab ab\nxx\nab");
        assert_eq!(found.iter().map(|hit| hit.line).collect::<Vec<_>>(), [0, 2]);
    }

    #[test]
    fn a_blank_query_finds_nothing_and_spaces_around_it_do_not_count() {
        assert!(hits("", "text").is_empty());
        assert!(hits("  ", "a  b").is_empty());
        assert_eq!(matched(&hits(" rust ", "I like rust.")), ["rust"]);
    }
}

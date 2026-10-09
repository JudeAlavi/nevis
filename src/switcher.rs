//! The quick switcher (Ctrl+P): find a file in the folder by typing a few of its letters; the
//! vault's headings and tags too, once something is typed (`#` first for tags).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::{gio, glib, prelude::*};

use crate::{listing, popup, vault};

/// Most results shown at once.
const SHOWN: usize = 50;
/// Files listed at most, so a huge folder can't stall the switcher.
const MAX_FILES: usize = 20_000;

/// The switcher popup, drawn over the window.
pub struct Switcher {
    pub widget: gtk::Box,
    entry: gtk::Entry,
    list: gtk::ListBox,
    root: RefCell<Option<gio::File>>,
    /// Files under the root, relative to it.
    paths: RefCell<Vec<String>>,
    /// What the list shows, in order.
    shown: RefCell<Vec<Found>>,
    /// The vault the folder is in, and its notes' headings and tags (read in the background).
    vault: RefCell<Option<PathBuf>>,
    index: RefCell<Option<Rc<Vec<vault::Note>>>>,
    pick: Box<dyn Fn(gio::File, Option<String>)>,
    tag: Box<dyn Fn(String)>,
}

/// A row: a file (in `paths`), a heading of a note (its path in the vault, its text), or a tag.
enum Found {
    File(usize),
    Heading(String, String),
    Tag(String),
    /// A note (its path) by one of its aliases.
    Alias(String, String),
}

impl Switcher {
    /// `pick` is called with the file the user chose (and the heading in it, if one was);
    /// `tag` with a tag chosen, to search for.
    pub fn new(pick: impl Fn(gio::File, Option<String>) + 'static, tag: impl Fn(String) + 'static) -> Rc<Self> {
        let entry = gtk::Entry::builder().placeholder_text("Go to file").build();
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).build();
        list.add_css_class("nevis-switcher-list");
        let (widget, card) = popup::card(560);
        card.append(&entry);
        card.append(&list);
        popup::browse_with_arrows(&widget, &list, &entry);

        let switcher = Rc::new(Self {
            widget,
            entry,
            list,
            root: RefCell::new(None),
            paths: RefCell::new(Vec::new()),
            shown: RefCell::new(Vec::new()),
            vault: RefCell::new(None),
            index: RefCell::new(None),
            pick: Box::new(pick),
            tag: Box::new(tag),
        });

        let weak = Rc::downgrade(&switcher);
        switcher.entry.connect_changed(move |_| {
            if let Some(s) = weak.upgrade() {
                s.refresh();
            }
        });
        let weak = Rc::downgrade(&switcher);
        switcher.entry.connect_activate(move |_| {
            if let Some(s) = weak.upgrade() {
                s.choose_selected();
            }
        });
        let weak = Rc::downgrade(&switcher);
        switcher.list.connect_row_activated(move |_, row| {
            if let Some(s) = weak.upgrade() {
                s.list.select_row(Some(row));
                s.choose_selected();
            }
        });
        switcher
    }

    pub fn is_open(&self) -> bool {
        self.widget.is_visible()
    }

    /// Opens the switcher on the files under `root`.
    pub fn show(self: &Rc<Self>, root: &gio::File) {
        let fresh = self.root.borrow().as_ref().is_none_or(|r| !r.equal(root));
        *self.root.borrow_mut() = Some(root.clone());
        if fresh {
            self.paths.borrow_mut().clear();
        }
        self.entry.set_text("");
        self.widget.set_visible(true);
        self.entry.grab_focus();
        self.refresh();
        // List the files in the background; the list fills in when that's done. The vault's
        // headings and tags too.
        let Some(dir) = root.path() else { return };
        let vault_root = vault::root_of(&dir, Some(&dir));
        let weak = Rc::downgrade(self);
        *self.index.borrow_mut() = vault::index(&vault_root, move |notes| {
            if let Some(s) = weak.upgrade() {
                *s.index.borrow_mut() = Some(notes);
                s.refresh();
            }
        });
        *self.vault.borrow_mut() = Some(vault_root);
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Ok(paths) = gio::spawn_blocking(move || files_under(&dir)).await else { return };
            if let Some(s) = weak.upgrade() {
                *s.paths.borrow_mut() = paths;
                s.refresh();
            }
        });
    }

    pub fn hide(&self) {
        self.widget.set_visible(false);
    }

    fn refresh(&self) {
        let query = self.entry.text().to_string();
        let paths = self.paths.borrow();
        let mut shown: Vec<Found> = Vec::new();
        // Tags first when the query starts with "#"; files then, and headings once typing.
        let tagging = query.strip_prefix('#');
        let index = self.index.borrow();
        let tags = || {
            let mut counts: HashMap<&str, usize> = HashMap::new();
            for note in index.iter().flat_map(|notes| notes.iter()) {
                for tag in &note.tags {
                    *counts.entry(tag.as_str()).or_default() += 1;
                }
            }
            let mut tags: Vec<(&str, usize)> = counts.into_iter().collect();
            tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            tags.into_iter().map(|(tag, _)| tag.trim_start_matches('#').to_owned()).collect::<Vec<_>>()
        };
        if let Some(tag) = tagging {
            let tags = tags();
            shown.extend(rank(tag, &tags).into_iter().take(SHOWN).map(|i| Found::Tag(format!("#{}", tags[i]))));
        }
        let files = if query.is_empty() { SHOWN } else { 30 };
        shown.extend(rank(&query, &paths).into_iter().take(files).map(Found::File));
        if !query.is_empty() && tagging.is_none() {
            let headings: Vec<(&str, &str)> = index
                .iter()
                .flat_map(|notes| notes.iter())
                .flat_map(|note| note.headings.iter().map(|heading| (note.path.as_str(), heading.as_str())))
                .collect();
            let texts: Vec<String> = headings.iter().map(|(_, heading)| heading.to_string()).collect();
            shown.extend(rank(&query, &texts).into_iter().take(15).map(|i| Found::Heading(headings[i].0.to_owned(), headings[i].1.to_owned())));
            let aliases: Vec<(&str, &str)> = index
                .iter()
                .flat_map(|notes| notes.iter())
                .flat_map(|note| note.aliases.iter().map(|alias| (note.path.as_str(), alias.as_str())))
                .collect();
            let words: Vec<String> = aliases.iter().map(|(_, alias)| alias.to_string()).collect();
            shown.extend(rank(&query, &words).into_iter().take(10).map(|i| Found::Alias(aliases[i].0.to_owned(), aliases[i].1.to_owned())));
            let tags = tags();
            shown.extend(rank(&query, &tags).into_iter().take(5).map(|i| Found::Tag(format!("#{}", tags[i]))));
        }
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        for found in &shown {
            let markup = match found {
                Found::File(index) => row_markup(&paths[*index]),
                Found::Heading(path, heading) => {
                    let note = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md");
                    format!(
                        "<span alpha=\"55%\">#</span> {}  <span alpha=\"55%\">{}</span>",
                        glib::markup_escape_text(heading),
                        glib::markup_escape_text(note)
                    )
                }
                Found::Tag(tag) => glib::markup_escape_text(tag).to_string(),
                Found::Alias(path, alias) => {
                    let note = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md");
                    format!("{}  <span alpha=\"55%\">\u{2192} {}</span>", glib::markup_escape_text(alias), glib::markup_escape_text(note))
                }
            };
            let label = gtk::Label::builder()
                .use_markup(true)
                .label(markup)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .build();
            self.list.append(&label);
        }
        self.list.set_visible(!shown.is_empty());
        self.list.select_row(self.list.row_at_index(0).as_ref());
        drop((paths, index));
        *self.shown.borrow_mut() = shown;
    }

    fn choose_selected(&self) {
        let Some(row) = self.list.selected_row() else { return };
        let chosen = match self.shown.borrow().get(row.index() as usize) {
            Some(Found::File(index)) => {
                let path = self.paths.borrow()[*index].clone();
                self.root.borrow().as_ref().map(|root| (root.resolve_relative_path(path), None))
            }
            Some(Found::Heading(path, heading)) => {
                self.vault.borrow().as_ref().map(|vault| (gio::File::for_path(vault.join(path)), Some(heading.clone())))
            }
            Some(Found::Alias(path, _)) => self.vault.borrow().as_ref().map(|vault| (gio::File::for_path(vault.join(path)), None)),
            Some(Found::Tag(tag)) => {
                let tag = tag.clone();
                self.hide();
                return (self.tag)(tag);
            }
            None => None,
        };
        if let Some((file, heading)) = chosen {
            self.hide();
            (self.pick)(file, heading);
        }
    }
}

/// "name  folder/", the folder dimmed; ".md" left out like in the sidebar.
pub fn row_markup(path: &str) -> String {
    let (folder, name) = path.rsplit_once('/').map_or(("", path), |(f, n)| (f, n));
    let name = name.strip_suffix(".md").unwrap_or(name);
    let mut markup = glib::markup_escape_text(name).to_string();
    if !folder.is_empty() {
        markup.push_str(&format!("  <span alpha=\"55%\">{}/</span>", glib::markup_escape_text(folder)));
    }
    markup
}

/// Text files under `root` (paths relative to it), skipping hidden folders and not following
/// links to folders, in folder order.
pub fn files_under(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            if !listing::shown(&name, is_dir, false) {
                continue;
            }
            if is_dir {
                folders.push(entry.path());
            } else if let Ok(relative) = entry.path().strip_prefix(root) {
                files.push(relative.to_string_lossy().into_owned());
                if files.len() >= MAX_FILES {
                    return files;
                }
            }
        }
    }
    files.sort_by(|a, b| listing::order((a, false), (b, false)));
    files
}

/// Indices of `paths` matching `query`, best match first; an empty query keeps them all,
/// in the order given.
pub fn rank(query: &str, paths: &[String]) -> Vec<usize> {
    if query.is_empty() {
        return (0..paths.len()).collect();
    }
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let mut scored: Vec<(usize, i64)> =
        (0..paths.len()).filter_map(|i| score(&query, &lower(&paths[i])).map(|s| (i, s))).collect();
    // Best score first; on a tie the shorter path, then the listing order (the sort is stable).
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| paths[a.0].len().cmp(&paths[b.0].len())));
    scored.into_iter().map(|(i, _)| i).collect()
}

/// Bonus for a query found entirely in the file's name rather than spread over its folders.
const IN_NAME: i64 = 1000;

/// How well `query` matches `path`, if at all; higher is better.
fn score(query: &[char], path: &[char]) -> Option<i64> {
    let name_start = path.iter().rposition(|&c| c == '/').map_or(0, |slash| slash + 1);
    match fit(query, &path[name_start..]) {
        Some(fit) => Some(IN_NAME + fit),
        None => fit(query, path),
    }
}

/// How tightly the query's letters sit in `text`: each letter counts, letters next to the
/// previous one count extra. Tries every place the first letter occurs and keeps the best.
fn fit(query: &[char], text: &[char]) -> Option<i64> {
    let Some(&first) = query.first() else { return Some(0) };
    (0..text.len())
        .filter(|&start| text[start] == first)
        .filter_map(|start| {
            let mut total = 10;
            let mut at = start;
            for &letter in &query[1..] {
                let next = at + 1 + text[at + 1..].iter().position(|&c| c == letter)?;
                total += if next == at + 1 { 25 } else { 10 };
                at = next;
            }
            Some(total)
        })
        .max()
}

fn lower(text: &str) -> Vec<char> {
    text.chars().flat_map(char::to_lowercase).collect()
}


#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn leaves_out_paths_that_do_not_match_and_ignores_case() {
        assert_eq!(rank("rdm", &paths(&["README.md", "notes/todo.md"])), vec![0]);
    }

    #[test]
    fn a_match_in_the_name_beats_one_in_the_folders() {
        assert_eq!(rank("todo", &paths(&["todo/a.md", "notes/todo.md"])), vec![1, 0]);
    }

    #[test]
    fn letters_typed_together_rank_higher() {
        assert_eq!(rank("note", &paths(&["n-o-t-e.md", "notes.md"])), vec![1, 0]);
    }

    #[test]
    fn on_a_tie_the_shorter_path_wins() {
        assert_eq!(rank("a", &paths(&["folder/a.md", "a.md"])), vec![1, 0]);
    }

    #[test]
    fn persian_queries_match_persian_names() {
        assert_eq!(rank("یاد", &paths(&["کتاب.md", "یادداشت.md"])), vec![1]);
    }

    #[test]
    fn an_empty_query_lists_everything_in_folder_order() {
        assert_eq!(rank("", &paths(&["notes/long.md", "a.md"])), vec![0, 1]);
    }
}

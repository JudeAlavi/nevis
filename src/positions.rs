//! Where the cursor was in each file, so reopening a file puts you back there.

use std::cell::RefCell;
use std::path::PathBuf;

use gtk::glib;

/// Files remembered at most; the least recently used are forgotten first.
pub const LIMIT: usize = 500;

thread_local! {
    /// The store, read from disk on first use and shared by all windows.
    static STORE: RefCell<Option<Positions>> = const { RefCell::new(None) };
}

/// Where the cursor was in `path` when it was last closed: (line, column).
pub fn recall(path: &str) -> Option<(u32, u32)> {
    with_store(|store| store.get(path))
}

/// Remembers where the cursor is in `path`, and saves the store.
pub fn keep(path: &str, line: u32, column: u32) {
    let text = with_store(|store| {
        store.remember(path, line, column);
        store.to_text()
    });
    let file = store_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Written aside and renamed into place, so a crash never leaves half a file.
    let partial = file.with_extension("partial");
    if std::fs::write(&partial, text).is_ok() {
        let _ = std::fs::rename(&partial, &file);
    }
}

fn with_store<T>(f: impl FnOnce(&mut Positions) -> T) -> T {
    STORE.with_borrow_mut(|slot| {
        f(slot.get_or_insert_with(|| Positions::parse(&std::fs::read_to_string(store_file()).unwrap_or_default())))
    })
}

fn store_file() -> PathBuf {
    state_dir().join("positions")
}

/// nevis's folder for remembered state: `$XDG_STATE_HOME/nevis`, by default
/// `~/.local/state/nevis`.
pub fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .unwrap_or_else(|| glib::home_dir().join(".local").join("state"));
    base.join("nevis")
}

/// Cursor positions by file path, most recently used first.
#[derive(Debug, Default, PartialEq)]
pub struct Positions(Vec<Entry>);

#[derive(Debug, PartialEq)]
struct Entry {
    path: String,
    line: u32,
    column: u32,
}

impl Positions {
    /// Reads the stored form: one "line\tcolumn\tpath" per line, the path last so it may
    /// itself contain tabs.
    pub fn parse(text: &str) -> Self {
        let entries = text
            .lines()
            .filter_map(|row| {
                let mut fields = row.splitn(3, '\t');
                let line = fields.next()?.parse().ok()?;
                let column = fields.next()?.parse().ok()?;
                let path = fields.next()?;
                Some(Entry { path: path.to_owned(), line, column })
            })
            .collect();
        Self(entries)
    }

    pub fn to_text(&self) -> String {
        self.0.iter().map(|e| format!("{}\t{}\t{}\n", e.line, e.column, e.path)).collect()
    }

    pub fn get(&self, path: &str) -> Option<(u32, u32)> {
        self.0.iter().find(|e| e.path == path).map(|e| (e.line, e.column))
    }

    pub fn remember(&mut self, path: &str, line: u32, column: u32) {
        self.0.retain(|e| e.path != path);
        self.0.insert(0, Entry { path: path.to_owned(), line, column });
        self.0.truncate(LIMIT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_where_the_cursor_was() {
        let mut positions = Positions::default();
        positions.remember("/notes/a.md", 12, 4);
        assert_eq!(positions.get("/notes/a.md"), Some((12, 4)));
        assert_eq!(positions.get("/notes/b.md"), None);
    }

    #[test]
    fn survives_a_round_trip_through_its_file() {
        let mut positions = Positions::default();
        positions.remember("/notes/a.md", 12, 4);
        positions.remember("/notes/b.md", 1, 0);
        let text = positions.to_text();
        assert_eq!(text, "1\t0\t/notes/b.md\n12\t4\t/notes/a.md\n");
        assert_eq!(Positions::parse(&text), positions);
    }

    #[test]
    fn remembering_a_file_again_moves_it_to_the_front_once() {
        let mut positions = Positions::default();
        positions.remember("/a.md", 1, 0);
        positions.remember("/b.md", 2, 0);
        positions.remember("/a.md", 9, 3);
        assert_eq!(positions.to_text(), "9\t3\t/a.md\n2\t0\t/b.md\n");
    }

    #[test]
    fn forgets_the_least_recently_used_beyond_the_limit() {
        let mut positions = Positions::default();
        for n in 0..=LIMIT {
            positions.remember(&format!("/{n}.md"), 1, 0);
        }
        assert_eq!(positions.to_text().lines().count(), LIMIT);
        assert_eq!(positions.get("/0.md"), None);
        assert_eq!(positions.get(&format!("/{LIMIT}.md")), Some((1, 0)));
    }

    #[test]
    fn skips_broken_lines_in_its_file() {
        let positions = Positions::parse("x\t1\t/bad-line.md\n5\t/no-column.md\n\n7\t2\t/ok.md\n");
        assert_eq!(positions.to_text(), "7\t2\t/ok.md\n");
    }

    #[test]
    fn keeps_paths_with_tabs_and_persian_intact() {
        let mut positions = Positions::default();
        positions.remember("/یادداشت‌ها/a\tb.md", 3, 1);
        let reread = Positions::parse(&positions.to_text());
        assert_eq!(reread.get("/یادداشت‌ها/a\tb.md"), Some((3, 1)));
    }
}

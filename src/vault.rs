//! A vault: the folder of notes that links are followed in. As in Obsidian, that's the folder
//! holding `.obsidian`, or else the folder the window shows.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::{gio, glib};
use pulldown_cmark::{Event, Tag};

use crate::{links, listing, obsidian, outline, render};

/// Notes bigger than this aren't read for the index (nobody writes a 4 MB note by hand).
const BIGGEST: u64 = 4 << 20;

/// A note's headings and tags, to go to (Ctrl+P) and to complete as they are typed.
pub struct Note {
    pub path: String,
    pub headings: Vec<String>,
    pub tags: Vec<String>,
    /// Other names links may call it by (its front matter's `aliases`).
    pub aliases: Vec<String>,
}

/// More files than this aren't listed (a vault that big is someone's home folder).
const MOST: usize = 50_000;

/// What was read of each vault, and when: kept for a while, as reading takes a moment.
type Kept<T> = RefCell<HashMap<PathBuf, (Instant, Rc<Vec<T>>)>>;

thread_local! {
    static FILES: Kept<String> = RefCell::new(HashMap::new());
    static INDEXES: Kept<Note> = RefCell::new(HashMap::new());
    static READING: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
}

/// The vault a note in `note_folder` belongs to: the nearest folder holding `.obsidian`, else
/// the window's folder when the note is inside it, else the note's own folder.
pub fn root_of(note_folder: &Path, window_folder: Option<&Path>) -> PathBuf {
    note_folder
        .ancestors()
        .find(|folder| folder.join(".obsidian").is_dir())
        .or_else(|| window_folder.filter(|folder| note_folder.starts_with(folder)))
        .unwrap_or(note_folder)
        .to_path_buf()
}

/// Every file in the vault at `root`, relative to it with `/` between folders, hidden ones left
/// out. Kept for a few seconds: drawing a note follows all its links at once.
pub fn files(root: &Path) -> Rc<Vec<String>> {
    let kept = FILES.with_borrow(|kept| {
        kept.get(root).filter(|(when, _)| when.elapsed() < Duration::from_secs(5)).map(|(_, files)| files.clone())
    });
    kept.unwrap_or_else(|| {
        let files = Rc::new(walk(root));
        FILES.with_borrow_mut(|kept| kept.insert(root.to_path_buf(), (Instant::now(), files.clone())));
        files
    })
}

/// Forgets the listings: nevis made, renamed or removed a file.
pub fn forget() {
    FILES.with_borrow_mut(HashMap::clear);
    INDEXES.with_borrow_mut(HashMap::clear);
}

/// The notes of the vault at `root` with their headings and tags, as last read. Older than
/// half a minute (or never read), they are read again in the background, and `ready` runs with
/// them then; meanwhile the old ones, if any, are returned.
pub fn index(root: &Path, ready: impl FnOnce(Rc<Vec<Note>>) + 'static) -> Option<Rc<Vec<Note>>> {
    let kept = INDEXES.with_borrow(|kept| kept.get(root).map(|(when, notes)| (when.elapsed(), notes.clone())));
    if let Some((age, notes)) = &kept
        && *age < Duration::from_secs(30)
    {
        return Some(notes.clone());
    }
    if READING.with_borrow_mut(|reading| reading.insert(root.to_path_buf())) {
        let notes: Vec<String> = files(root).iter().filter(|path| listing::is_note(path)).cloned().collect();
        let (dir, key) = (root.to_path_buf(), root.to_path_buf());
        glib::spawn_future_local(async move {
            let read = gio::spawn_blocking(move || read_notes(&dir, &notes)).await;
            READING.with_borrow_mut(|reading| reading.remove(&key));
            let Ok(read) = read else { return };
            let read = Rc::new(read);
            INDEXES.with_borrow_mut(|kept| kept.insert(key, (Instant::now(), read.clone())));
            ready(read);
        });
    }
    kept.map(|(_, notes)| notes)
}

/// Each note's headings and tags, read on a few threads.
fn read_notes(root: &Path, notes: &[String]) -> Vec<Note> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(4));
    let share = notes.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let running: Vec<_> = notes
            .chunks(share)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .filter_map(|path| {
                            let on_disk = root.join(path);
                            if std::fs::metadata(&on_disk).is_ok_and(|m| m.len() > BIGGEST) {
                                return None;
                            }
                            let text = std::fs::read_to_string(&on_disk).ok()?;
                            let headings = outline::headings(&text).into_iter().map(|heading| heading.text).collect();
                            let mut tags: Vec<String> = obsidian::tags(&text).into_iter().map(|range| text[range].to_owned()).collect();
                            tags.sort();
                            tags.dedup();
                            let aliases = obsidian::aliases(&text);
                            Some(Note { path: path.clone(), headings, tags, aliases })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        running.into_iter().flat_map(|thread| thread.join().unwrap_or_default()).collect()
    })
}

fn walk(root: &Path) -> Vec<String> {
    let (mut files, mut folders) = (Vec::new(), vec![root.to_path_buf()]);
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            // Links to folders aren't followed: they could loop.
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => folders.push(entry.path()),
                Ok(_) => {
                    if let Ok(relative) = entry.path().strip_prefix(root) {
                        files.push(relative.to_string_lossy().replace(MAIN_SEPARATOR, "/"));
                    }
                }
                Err(_) => {}
            }
            if files.len() >= MOST {
                return files;
            }
        }
    }
    files
}

/// Which of the vault's `files` a link opens. `target` is the link's path part, decoded
/// (`Plans`, `Trips/Sicily.md`, `Figure 1.png`); `from` is the folder of the note holding the
/// link, relative to the vault (`""` at its top). Paths use `/`.
pub fn resolve(target: &str, from: &str, files: &[String]) -> Option<String> {
    // A note may be named without its .md; other files keep their extension.
    let mut names = vec![target.to_owned()];
    if !target.to_lowercase().ends_with(".md") {
        names.push(format!("{target}.md"));
    }
    let listed = |path: &str| {
        let path = path.to_lowercase();
        files.iter().find(|file| file.to_lowercase() == path).cloned()
    };
    // Beside the note first: the path followed from its folder ("/" starts at the top).
    for name in &names {
        let followed = match name.strip_prefix('/') {
            Some(from_top) => join("", from_top),
            None => join(from, name),
        };
        if let Some(found) = followed.as_deref().and_then(listed) {
            return Some(found);
        }
    }
    if target.starts_with('/') || target.starts_with("./") || target.starts_with("../") {
        return None;
    }
    // Then anywhere: a file whose path ends with the link's, in whole folder names; the
    // shortest path wins a tie.
    for name in &names {
        let name = name.to_lowercase();
        let ending = format!("/{name}");
        let best = files
            .iter()
            .filter(|file| {
                let file = file.to_lowercase();
                file == name || file.ends_with(&ending)
            })
            .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
        if let Some(best) = best {
            return Some(best.clone());
        }
    }
    None
}

/// `path` followed from `folder`, `.` and `..` worked out; `None` when it is absolute or
/// climbs above the top.
pub fn join(folder: &str, path: &str) -> Option<String> {
    if path.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = folder.split('/').filter(|part| !part.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// `text`, the note at `from`, with its links to the note at `old` pointing at `new` instead
/// (renamed; `files` as they were before), each keeping its heading, its words and the way it
/// is written; `None` when it has no link to `old`. Paths are relative to the vault.
pub fn relink(text: &str, from: &str, old: &str, new: &str, files: &[String]) -> Option<String> {
    let folder = from.rsplit_once('/').map_or("", |(folder, _)| folder);
    let name = new.rsplit('/').next().unwrap_or(new);
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for (event, range) in render::events(text) {
        let (Event::Start(Tag::Link { dest_url, .. }) | Event::Start(Tag::Image { dest_url, .. })) = event else { continue };
        let path = links::split(&dest_url).path;
        if links::has_scheme(&dest_url) || path.is_empty() || resolve(&path, folder, files).as_deref() != Some(old) {
            continue;
        }
        if let Some(edit) = renamed_in(&text[range.clone()], name) {
            edits.push((range, edit));
        }
    }
    if edits.is_empty() {
        return None;
    }
    let mut text = text.to_owned();
    for (range, edit) in edits.into_iter().rev() {
        text.replace_range(range, &edit);
    }
    Some(text)
}

/// A link's source (`[[Plans#H|words]]`, `![[Plans]]`, `[words](../Plans.md#H)`) with the
/// file it names now called `name`: folders, heading and words kept, written the same way.
fn renamed_in(source: &str, name: &str) -> Option<String> {
    let stem = name.strip_suffix(".md").unwrap_or(name);
    // The last part of a path, renamed: with .md only if it had it.
    let rename = |path: &str| {
        let (folders, last) = path.rsplit_once('/').map_or(("", path), |(folders, last)| (folders, last));
        let last = if last.to_lowercase().ends_with(".md") { name } else { stem };
        if folders.is_empty() && !path.starts_with('/') { last.to_owned() } else { format!("{folders}/{last}") }
    };
    if let Some(prefix) = ["![[", "[["].into_iter().find(|prefix| source.starts_with(prefix)) {
        let inner = source.strip_prefix(prefix)?.strip_suffix("]]")?;
        let end = inner.find(['#', '|']).unwrap_or(inner.len());
        return Some(format!("{prefix}{}{}]]", rename(&inner[..end]), &inner[end..]));
    }
    // [words](destination "title"): the destination, maybe in <…>, before any #heading.
    let open = source.rfind("](")? + 2;
    let inside = source.strip_suffix(')')?.get(open..)?;
    let (destination, after) = match inside.strip_prefix('<') {
        Some(angled) => {
            let close = angled.find('>')?;
            (&angled[..close], &angled[close + 1..])
        }
        None => inside.split_at(inside.find(char::is_whitespace).unwrap_or(inside.len())),
    };
    let end = destination.find('#').unwrap_or(destination.len());
    let path = rename(&destination[..end]);
    let written = if inside.starts_with('<') { format!("<{path}{}>", &destination[end..]) } else { format!("{}{}", path.replace(' ', "%20"), &destination[end..]) };
    Some(format!("{}{written}{after})", &source[..open]))
}

/// The note `target` (a link's path, its `.md` aside) is another name of, by `notes`' aliases,
/// case aside: for a link no file's name matches, as Obsidian falls back to aliases.
pub fn by_alias(target: &str, notes: &[Note]) -> Option<String> {
    let name = target.strip_suffix(".md").unwrap_or(target).to_lowercase();
    notes.iter().find(|note| note.aliases.iter().any(|alias| alias.to_lowercase() == name)).map(|note| note.path.clone())
}

/// The aliases of the note at `path` under `root`, as the vault's index has them (none before
/// it is first read).
pub fn aliases_of(root: &Path, path: &str) -> Vec<String> {
    index(root, |_| {}).and_then(|notes| notes.iter().find(|note| note.path == path).map(|note| note.aliases.clone())).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(path: &str, aliases: &[&str]) -> Note {
        Note { path: path.to_owned(), headings: Vec::new(), tags: Vec::new(), aliases: aliases.iter().map(|alias| alias.to_string()).collect() }
    }

    #[test]
    fn a_name_no_file_has_is_looked_for_among_aliases() {
        let notes = vec![note("Other.md", &[]), note("Trips/Plan.md", &["Sicily trip", "Trip 2026"])];
        assert_eq!(by_alias("sicily TRIP", &notes).as_deref(), Some("Trips/Plan.md"));
        assert_eq!(by_alias("Trip 2026.md", &notes).as_deref(), Some("Trips/Plan.md"));
        assert_eq!(by_alias("Nowhere", &notes), None);
    }

    fn files(list: &[&str]) -> Vec<String> {
        list.iter().map(|path| path.to_string()).collect()
    }

    #[test]
    fn links_to_a_renamed_note_follow_it_keeping_headings_and_words() {
        let vault = files(&["Plans.md", "Daily/Note.md", "Other.md"]);
        let text = "[[Plans]] and [[Plans#Budget|the budget]]\n[x](Plans.md) [y](../Plans.md#Dates) ![[Plans]]\n[[Other]] `[[Plans]]`\n";
        assert_eq!(
            relink(text, "Daily/Note.md", "Plans.md", "Trip plans.md", &vault).as_deref(),
            Some("[[Trip plans]] and [[Trip plans#Budget|the budget]]\n[x](Trip%20plans.md) [y](../Trip%20plans.md#Dates) ![[Trip plans]]\n[[Other]] `[[Plans]]`\n")
        );
    }

    #[test]
    fn a_path_keeps_its_folders_and_angle_brackets_keep_spaces() {
        let vault = files(&["Projects/Plans.md", "Plans.md", "Note.md"]);
        let text = "[[Projects/Plans]] [a](<Projects/Plans.md>) [[Plans]]\n";
        assert_eq!(
            relink(text, "Note.md", "Projects/Plans.md", "Projects/New plans.md", &vault).as_deref(),
            Some("[[Projects/New plans]] [a](<Projects/New plans.md>) [[Plans]]\n")
        );
    }

    #[test]
    fn a_note_with_no_link_to_it_is_left_alone() {
        let vault = files(&["Plans.md", "Other.md", "Note.md"]);
        assert_eq!(relink("[[Other]] and plans\n", "Note.md", "Plans.md", "New.md", &vault), None);
    }

    #[test]
    fn a_link_beside_the_note_comes_first() {
        let vault = files(&["Plans.md", "Daily/Plans.md"]);
        assert_eq!(resolve("Plans", "Daily", &vault).as_deref(), Some("Daily/Plans.md"));
        assert_eq!(resolve("Plans", "", &vault).as_deref(), Some("Plans.md"));
    }

    #[test]
    fn otherwise_any_note_of_that_name_in_the_vault_case_aside() {
        let vault = files(&["1 - Rough Notes/Progress.md", "3 - Tags/algorithm.md"]);
        assert_eq!(resolve("algorithm", "1 - Rough Notes", &vault).as_deref(), Some("3 - Tags/algorithm.md"));
        assert_eq!(resolve("Algorithm", "1 - Rough Notes", &vault).as_deref(), Some("3 - Tags/algorithm.md"));
        assert_eq!(resolve("algorithm.md", "", &vault).as_deref(), Some("3 - Tags/algorithm.md"));
    }

    #[test]
    fn a_path_matches_whole_folder_names_and_the_shortest_path_wins_a_tie() {
        let vault = files(&["a/b/Note.md", "c/Note.md", "3 - Tags/algorithm.md"]);
        assert_eq!(resolve("Note", "x", &vault).as_deref(), Some("c/Note.md"));
        assert_eq!(resolve("b/Note", "x", &vault).as_deref(), Some("a/b/Note.md"));
        assert_eq!(resolve("Tags/algorithm", "x", &vault), None);
    }

    #[test]
    fn other_files_need_their_extension_and_dotted_names_still_find_notes() {
        let vault = files(&["img/Figure 1.png", "v1.2 notes.md"]);
        assert_eq!(resolve("Figure 1.png", "notes", &vault).as_deref(), Some("img/Figure 1.png"));
        assert_eq!(resolve("Figure 1", "notes", &vault), None);
        assert_eq!(resolve("v1.2 notes", "", &vault).as_deref(), Some("v1.2 notes.md"));
    }

    #[test]
    fn relative_paths_climb_from_the_notes_folder() {
        let vault = files(&["Plans.md", "Daily/Trips/Sicily.md"]);
        assert_eq!(resolve("../Plans.md", "Daily", &vault).as_deref(), Some("Plans.md"));
        assert_eq!(resolve("./Trips/Sicily.md", "Daily", &vault).as_deref(), Some("Daily/Trips/Sicily.md"));
        assert_eq!(resolve("Nowhere", "Daily", &vault), None);
    }
}

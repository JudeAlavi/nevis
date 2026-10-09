//! Backlinks: the notes in a folder that link to a note, listed under it in the rendered view.

use std::path::Path;

use pulldown_cmark::{Event, Tag};

use crate::{links, listing, render, switcher};

/// Notes bigger than this aren't read (nobody writes a 4 MB note by hand).
const BIGGEST: u64 = 4 << 20;

/// A line in a note with a link to another note: its number, and its text.
#[derive(Debug, PartialEq)]
pub struct Mention {
    pub line: usize,
    pub text: String,
}

/// The lines of `text`, the note at `from`, with links that lead to the note at `to`: the
/// way nevis follows them, `[[Name]]` and relative links resolved from `from`'s folder.
/// Both paths are relative to the same folder, with "/" between their parts.
pub fn mentions(text: &str, from: &str, to: &str) -> Vec<Mention> {
    let folder = from.rsplit_once('/').map_or("", |(folder, _)| folder);
    let mut lines: Vec<usize> = Vec::new();
    for (event, range) in render::events(text) {
        let Event::Start(Tag::Link { dest_url, .. }) = event else { continue };
        if links::has_scheme(&dest_url) {
            continue;
        }
        let target = links::split(&dest_url);
        if target.path.is_empty() || resolve(folder, &target.path).as_deref() != Some(to) {
            continue;
        }
        let line = text[..range.start].matches('\n').count();
        if lines.last() != Some(&line) {
            lines.push(line);
        }
    }
    let all: Vec<&str> = text.lines().collect();
    lines.into_iter().map(|line| Mention { line, text: all.get(line).unwrap_or(&"").trim().to_owned() }).collect()
}

/// The notes under `root` that link to `to` (relative to `root`), with the lines that do, in
/// the order the folder lists them. Reads the notes on a few threads; call it off the main one.
pub fn find(root: &Path, to: &str) -> Vec<(String, Vec<Mention>)> {
    let files = switcher::files_under(root);
    // Only notes with the linked note's name in them, written either way, are read closely.
    let name = to.rsplit('/').next().unwrap_or(to);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let encoded = percent(stem);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(4));
    let share = files.len().div_ceil(threads).max(1);
    let (stem, encoded) = (stem, encoded.as_str());
    std::thread::scope(|scope| {
        let running: Vec<_> = files
            .chunks(share)
            .map(|part| {
                scope.spawn(move || {
                    let mut found = Vec::new();
                    for path in part.iter().filter(|path| path.as_str() != to && listing::is_note(path)) {
                        let on_disk = root.join(path);
                        if std::fs::metadata(&on_disk).is_ok_and(|m| m.len() > BIGGEST) {
                            continue;
                        }
                        let Ok(text) = std::fs::read_to_string(&on_disk) else { continue };
                        if !text.contains(stem) && !text.contains(encoded) {
                            continue;
                        }
                        let lines = mentions(&text, path, to);
                        if !lines.is_empty() {
                            found.push((path.clone(), lines));
                        }
                    }
                    found
                })
            })
            .collect();
        running.into_iter().flat_map(|thread| thread.join().unwrap_or_default()).collect()
    })
}

/// Spaces and letters beyond ASCII as %XX, the way some editors write link paths.
fn percent(text: &str) -> String {
    text.bytes()
        .map(|b| if b == b' ' || !b.is_ascii() { format!("%{b:02X}") } else { char::from(b).to_string() })
        .collect()
}

/// `path` followed from `folder`, `.` and `..` worked out; `None` when it is absolute or
/// climbs above the top.
fn resolve(folder: &str, path: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mention(line: usize, text: &str) -> Mention {
        Mention { line, text: text.to_owned() }
    }

    #[test]
    fn wiki_names_headings_and_relative_links_all_count() {
        let text = "See [[Plans]].\nAlso [[Plans#Budget|the budget]]\n[the plan](Plans.md) and [web](https://x.org)\n[[Other]]\n";
        assert_eq!(
            mentions(text, "Daily.md", "Plans.md"),
            vec![
                mention(0, "See [[Plans]]."),
                mention(1, "Also [[Plans#Budget|the budget]]"),
                mention(2, "[the plan](Plans.md) and [web](https://x.org)"),
            ]
        );
    }

    #[test]
    fn links_lead_from_the_linking_notes_folder() {
        let text = "[up](../Plans.md)\n[[Plans]]\n[down](./Trips/Sicily.md)\n";
        assert_eq!(mentions(text, "Daily/2026-10-08.md", "Plans.md"), vec![mention(0, "[up](../Plans.md)")]);
        assert_eq!(mentions(text, "Daily/2026-10-08.md", "Daily/Plans.md"), vec![mention(1, "[[Plans]]")]);
        assert_eq!(mentions(text, "Daily/2026-10-08.md", "Daily/Trips/Sicily.md"), vec![mention(2, "[down](./Trips/Sicily.md)")]);
    }

    #[test]
    fn code_does_not_count_and_names_match_however_they_are_written() {
        let text = "`[[My Plans]]`\n```\n[[My Plans]]\n```\n[x](My%20Plans.md) and [[My Plans]]\n[[برنامه\u{200c}ها]]\n";
        assert_eq!(mentions(text, "a.md", "My Plans.md"), vec![mention(4, "[x](My%20Plans.md) and [[My Plans]]")]);
        assert_eq!(mentions(text, "a.md", "برنامه\u{200c}ها.md"), vec![mention(5, "[[برنامه\u{200c}ها]]")]);
    }
}

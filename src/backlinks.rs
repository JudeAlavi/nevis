//! Backlinks: the notes in a folder that link to a note, listed under it in the rendered view.

use std::path::Path;

use pulldown_cmark::{Event, Tag};

use crate::{links, listing, render, vault};

/// Notes bigger than this aren't read (nobody writes a 4 MB note by hand).
const BIGGEST: u64 = 4 << 20;

/// A line in a note with a link to another note: its number, and its text.
#[derive(Debug, PartialEq)]
pub struct Mention {
    pub line: usize,
    pub text: String,
}

/// The lines of `text`, the note at `from`, with links that lead to the note at `to`, followed
/// the way a click follows them (`vault::resolve`) among the vault's `files`. Paths are
/// relative to the vault, with "/" between their parts.
pub fn mentions(text: &str, from: &str, to: &str, files: &[String], aliases: &[String]) -> Vec<Mention> {
    let folder = from.rsplit_once('/').map_or("", |(folder, _)| folder);
    let mut lines: Vec<usize> = Vec::new();
    for (event, range) in render::events(text) {
        let Event::Start(Tag::Link { dest_url, .. }) = event else { continue };
        if links::has_scheme(&dest_url) {
            continue;
        }
        let target = links::split(&dest_url);
        if target.path.is_empty() {
            continue;
        }
        // By a file's name, or else by one of the note's aliases (as Obsidian goes).
        let found = vault::resolve(&target.path, folder, files);
        let name = target.path.strip_suffix(".md").unwrap_or(&target.path).to_lowercase();
        let by_alias = found.is_none() && aliases.iter().any(|alias| alias.to_lowercase() == name);
        if found.as_deref() != Some(to) && !by_alias {
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

/// The notes among the vault's `files` (at `root`) that link to `to`, with the lines that do,
/// in the order listed. Reads the notes on a few threads; call it off the main one.
pub fn find(root: &Path, to: &str, files: &[String], aliases: &[String]) -> Vec<(String, Vec<Mention>)> {
    let notes: Vec<&String> = files.iter().filter(|path| listing::is_note(path)).collect();
    // Only notes with the linked note's name in them, written either way, are read closely.
    let name = to.rsplit('/').next().unwrap_or(to);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let encoded = percent(stem);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(4));
    let share = notes.len().div_ceil(threads).max(1);
    let (stem, encoded) = (stem, encoded.as_str());
    std::thread::scope(|scope| {
        let running: Vec<_> = notes
            .chunks(share)
            .map(|part| {
                scope.spawn(move || {
                    let mut found = Vec::new();
                    for &path in part.iter().filter(|path| path.as_str() != to) {
                        let on_disk = root.join(path);
                        if std::fs::metadata(&on_disk).is_ok_and(|m| m.len() > BIGGEST) {
                            continue;
                        }
                        let Ok(text) = std::fs::read_to_string(&on_disk) else { continue };
                        if !text.contains(stem) && !text.contains(encoded) && !aliases.iter().any(|alias| text.contains(alias.as_str())) {
                            continue;
                        }
                        let lines = mentions(&text, path, to, files, aliases);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mention(line: usize, text: &str) -> Mention {
        Mention { line, text: text.to_owned() }
    }

    fn vault(list: &[&str]) -> Vec<String> {
        list.iter().map(|path| path.to_string()).collect()
    }

    #[test]
    fn wiki_names_headings_and_relative_links_all_count() {
        let files = vault(&["Daily.md", "Plans.md", "Other.md"]);
        let text = "See [[Plans]].\nAlso [[Plans#Budget|the budget]]\n[the plan](Plans.md) and [web](https://x.org)\n[[Other]]\n";
        assert_eq!(
            mentions(text, "Daily.md", "Plans.md", &files, &[]),
            vec![
                mention(0, "See [[Plans]]."),
                mention(1, "Also [[Plans#Budget|the budget]]"),
                mention(2, "[the plan](Plans.md) and [web](https://x.org)"),
            ]
        );
    }

    #[test]
    fn links_lead_from_the_linking_notes_folder_first() {
        let files = vault(&["Plans.md", "Daily/Plans.md", "Daily/Trips/Sicily.md", "Daily/2026-10-08.md"]);
        let text = "[up](../Plans.md)\n[[Plans]]\n[down](./Trips/Sicily.md)\n";
        assert_eq!(mentions(text, "Daily/2026-10-08.md", "Plans.md", &files, &[]), vec![mention(0, "[up](../Plans.md)")]);
        assert_eq!(mentions(text, "Daily/2026-10-08.md", "Daily/Plans.md", &files, &[]), vec![mention(1, "[[Plans]]")]);
        assert_eq!(mentions(text, "Daily/2026-10-08.md", "Daily/Trips/Sicily.md", &files, &[]), vec![mention(2, "[down](./Trips/Sicily.md)")]);
    }

    #[test]
    fn a_link_by_one_of_the_notes_aliases_counts() {
        let files = vault(&["Trips/Plan.md", "Daily/Today.md", "Sicily.md"]);
        let text = "[[Sicily trip]] tonight\n[[Sicily]] is another note\n";
        let aliases = vec!["Sicily trip".to_owned()];
        assert_eq!(mentions(text, "Daily/Today.md", "Trips/Plan.md", &files, &aliases), vec![mention(0, "[[Sicily trip]] tonight")]);
    }

    #[test]
    fn a_name_finds_its_note_anywhere_in_the_vault_as_a_click_would() {
        let files = vault(&["1 - Rough Notes/Progress.md", "3 - Tags/algorithm.md"]);
        let text = "tags: [[algorithm]] [[Algorithm|algo]]\n";
        assert_eq!(
            mentions(text, "1 - Rough Notes/Progress.md", "3 - Tags/algorithm.md", &files, &[]),
            vec![mention(0, "tags: [[algorithm]] [[Algorithm|algo]]")]
        );
    }

    #[test]
    fn code_does_not_count_and_names_match_however_they_are_written() {
        let files = vault(&["a.md", "My Plans.md", "برنامه\u{200c}ها.md"]);
        let text = "`[[My Plans]]`\n```\n[[My Plans]]\n```\n[x](My%20Plans.md) and [[My Plans]]\n[[برنامه\u{200c}ها]]\n";
        assert_eq!(mentions(text, "a.md", "My Plans.md", &files, &[]), vec![mention(4, "[x](My%20Plans.md) and [[My Plans]]")]);
        assert_eq!(mentions(text, "a.md", "برنامه\u{200c}ها.md", &files, &[]), vec![mention(5, "[[برنامه\u{200c}ها]]")]);
    }
}

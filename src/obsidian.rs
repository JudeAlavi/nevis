//! Obsidian's additions to Markdown, as its help describes them: tags, and (to come)
//! highlights, comments, callouts and block ids.

use std::ops::Range;

use pulldown_cmark::{Event, Tag};

use crate::render;

/// The tags in `text` (`#tag`, `#nested/tag`, `#کتاب`), as byte ranges with their `#`: after
/// the start of a line or a space, letters, digits, `_`, `-` and `/`, at least one of them
/// not a digit; never in code.
pub fn tags(text: &str) -> Vec<Range<usize>> {
    tags_outside(text, &code(text))
}

fn tags_outside(text: &str, code: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut previous: Option<char> = None;
    for (at, c) in text.char_indices() {
        let after_space = previous.is_none_or(char::is_whitespace);
        previous = Some(c);
        if c != '#' || !after_space || code.iter().any(|range| range.contains(&at)) {
            continue;
        }
        let rest = &text[at + 1..];
        let mut chars = rest.char_indices().peekable();
        let (mut end, mut words) = (at + 1, false);
        while let Some((i, d)) = chars.next() {
            // A "/" nests a tag only when a tag letter follows it.
            let nesting = d == '/' && chars.peek().is_some_and(|&(_, next)| tag_letter(next));
            if !tag_letter(d) && !nesting {
                break;
            }
            words |= !d.is_numeric() && d != '/';
            end = at + 1 + i + d.len_utf8();
        }
        if words {
            found.push(at..end);
        }
    }
    found
}

/// An Obsidian callout, from the first line of its quote: `[!tip]- Read this`.
#[derive(Debug, PartialEq)]
pub struct Callout {
    /// Which of Obsidian's kinds it looks like (an unknown kind looks like a note).
    pub kind: &'static str,
    pub title: String,
    /// Foldable: open (`+`) or closed (`-`); `None` when it doesn't fold.
    pub fold: Option<bool>,
}

/// Obsidian's callout kinds, each with the other names it goes by.
const KINDS: [(&str, &[&str]); 13] = [
    ("note", &["note"]),
    ("abstract", &["abstract", "summary", "tldr"]),
    ("info", &["info"]),
    ("todo", &["todo"]),
    ("tip", &["tip", "hint", "important"]),
    ("success", &["success", "check", "done"]),
    ("question", &["question", "help", "faq"]),
    ("warning", &["warning", "caution", "attention"]),
    ("failure", &["failure", "fail", "missing"]),
    ("danger", &["danger", "error"]),
    ("bug", &["bug"]),
    ("example", &["example"]),
    ("quote", &["quote", "cite"]),
];

/// The callout a quote's first line (after its `>`) starts, if it starts one: `[!kind]`, a
/// `+` or `-` to fold, then the title (the kind, capitalised, when there's none). Text after
/// a `|` in the brackets is for styling, and left out.
pub fn callout(line: &str) -> Option<Callout> {
    let rest = line.trim_start().strip_prefix("[!")?;
    let close = rest.find(']')?;
    let written = rest[..close].split('|').next()?.trim();
    if written.is_empty() || !written.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    let after = &rest[close + 1..];
    let (fold, title) = match after.strip_prefix('+') {
        Some(title) => (Some(true), title),
        None => match after.strip_prefix('-') {
            Some(title) => (Some(false), title),
            None => (None, after),
        },
    };
    let lower = written.to_lowercase();
    let kind = KINDS.iter().find(|(_, names)| names.contains(&lower.as_str())).map_or("note", |(kind, _)| kind);
    let title = match title.trim() {
        "" => {
            let mut letters = lower.chars();
            letters.next().map(|first| first.to_uppercase().chain(letters).collect()).unwrap_or_default()
        }
        title => title.to_owned(),
    };
    Some(Callout { kind, title, fold })
}

/// The `==highlights==` in `text`, as byte ranges with their markers: on one line, with
/// something in them, never in code.
fn highlights_outside(text: &str, code: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let mut at = 0;
        while let Some(open) = line[at..].find("==").map(|i| at + i) {
            let Some(close) = line[open + 2..].find("==").map(|i| open + 2 + i) else { break };
            let inner = &line[open + 2..close];
            // Like emphasis: something in it, touching both markers.
            let marks = !inner.is_empty() && !inner.starts_with(char::is_whitespace) && !inner.ends_with(char::is_whitespace);
            let range = offset + open..offset + close + 2;
            if marks && !code.iter().any(|c| c.contains(&range.start)) {
                found.push(range);
                at = close + 2;
            } else {
                at = open + 2;
            }
        }
        offset += line.len();
    }
    found
}

/// The `%%comments%%` in `text`, as byte ranges with their markers: from a `%%` to the next,
/// across lines too; one that isn't closed runs to the end. `\%%` and code don't count.
fn comments_outside(text: &str, code: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut markers = Vec::new();
    let mut at = 0;
    while let Some(found) = text[at..].find("%%").map(|i| at + i) {
        let escaped = text[..found].ends_with('\\');
        if !escaped && !code.iter().any(|c| c.contains(&found)) {
            markers.push(found);
        }
        at = found + 2;
    }
    markers
        .chunks(2)
        .map(|pair| match pair {
            [open, close] => *open..close + 2,
            [open] => *open..text.len(),
            _ => unreachable!("chunks of two"),
        })
        .collect()
}

/// The block ids in `text` (`^id` ending a line, after a space or alone on it), as byte ranges.
fn block_ids_outside(text: &str, code: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end();
        if let Some(caret) = content.rfind('^') {
            let (before, id) = (&content[..caret], &content[caret + 1..]);
            let spaced = before.is_empty() || before.ends_with([' ', '\t']);
            let start = offset + caret;
            if spaced
                && !id.is_empty()
                && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && !code.iter().any(|c| c.contains(&start))
            {
                found.push(start..start + 1 + id.len());
            }
        }
        offset += line.len();
    }
    found
}

/// A note's properties (its front matter, without the `---` lines), as Obsidian lists them:
/// each name with its values (`key: value`, `key: [a, b]`, or `- item` lines under it).
pub fn properties(yaml: &str) -> Vec<(String, Vec<String>)> {
    let unquote = |value: &str| {
        let value = value.trim();
        let quoted = value.len() >= 2 && ((value.starts_with('"') && value.ends_with('"')) || (value.starts_with('\'') && value.ends_with('\'')));
        if quoted { value[1..value.len() - 1].to_owned() } else { value.to_owned() }
    };
    let mut found: Vec<(String, Vec<String>)> = Vec::new();
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // "- item" lines belong to the name above them.
        if let Some(item) = trimmed.strip_prefix('-').filter(|_| line.starts_with([' ', '\t', '-'])) {
            if let Some((_, values)) = found.last_mut()
                && !item.trim().is_empty()
            {
                values.push(unquote(item));
            }
            continue;
        }
        let Some((name, value)) = trimmed.split_once(':') else { continue };
        let value = value.trim();
        let values = match value.strip_prefix('[').and_then(|list| list.strip_suffix(']')) {
            Some(list) => list.split(',').map(unquote).filter(|item| !item.is_empty()).collect(),
            None if value.is_empty() => Vec::new(),
            None => vec![unquote(value)],
        };
        found.push((name.trim().to_owned(), values));
    }
    found
}

/// A note's aliases (`aliases:`, or `alias:`, in its front matter): other names links may
/// call it by.
pub fn aliases(text: &str) -> Vec<String> {
    let end = render::front_matter(text);
    if end == 0 {
        return Vec::new();
    }
    // Between its first line and its last (the `---`s).
    let mut lines: Vec<&str> = text[..end].split_inclusive('\n').collect();
    lines.pop();
    let yaml: String = lines.into_iter().skip(1).collect();
    properties(&yaml)
        .into_iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("aliases") || name.eq_ignore_ascii_case("alias"))
        .flat_map(|(_, values)| values)
        .collect()
}

/// The block `^id` names in `text` (an id in code doesn't count): the paragraph it ends, the
/// list item it ends, or, alone on its line, the block above it. Byte range.
pub fn block(text: &str, id: &str) -> Option<Range<usize>> {
    let found = block_ids_outside(text, &code(text)).into_iter().find(|range| text[range.start + 1..range.end] == *id)?;
    let line = line_around(text, found.start);
    let words = text[line.clone()].trim_start();
    // Alone on its line: the block above, past any blank lines.
    if words.trim_end() == &text[found] {
        let mut above = line.start;
        loop {
            let previous = line_around(text, above.checked_sub(1)?);
            if !text[previous.clone()].trim().is_empty() {
                return Some(paragraph_start(text, previous.start)..previous.end);
            }
            above = previous.start;
        }
    }
    let ordered = words.split_once(['.', ')']).is_some_and(|(number, rest)| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) && rest.starts_with(' '));
    let item = (words.starts_with(['-', '*', '+']) && words[1..].starts_with(' ')) || ordered;
    if item { Some(line) } else { Some(paragraph_start(text, line.start)..line.end) }
}

/// The line `at` is on (without its end), as a byte range.
fn line_around(text: &str, at: usize) -> Range<usize> {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    start..end
}

/// Where the paragraph with its line starting at `line` starts: after the blank line before it.
fn paragraph_start(text: &str, mut line: usize) -> usize {
    while line > 0 {
        let previous = line_around(text, line - 1);
        if text[previous.clone()].trim().is_empty() {
            break;
        }
        line = previous.start;
    }
    line
}

/// Obsidian's additions in a note, read with its code found once.
pub struct Additions {
    pub highlights: Vec<Range<usize>>,
    pub tags: Vec<Range<usize>>,
    pub comments: Vec<Range<usize>>,
    pub block_ids: Vec<Range<usize>>,
}

/// All of Obsidian's additions in `text` (as `highlights`, `tags`, `comments`, `block_ids`).
pub fn additions(text: &str) -> Additions {
    additions_in(text, &code(text))
}

/// The same, given where `text`'s code and math are (its parse found them already): what is in
/// them doesn't count.
pub fn additions_in(text: &str, code: &[Range<usize>]) -> Additions {
    Additions {
        highlights: highlights_outside(text, code),
        tags: tags_outside(text, code),
        comments: comments_outside(text, code),
        block_ids: block_ids_outside(text, code),
    }
}

/// What a tag may be made of, besides "/": letters and digits of any script, "_" and "-",
/// combining marks, the Persian half-space, and emoji.
fn tag_letter(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(c, '_' | '-' | '\u{200C}' | '\u{200D}')
        || matches!(c, '\u{0300}'..='\u{036F}' | '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{FE0F}')
        || matches!(c, '\u{2600}'..='\u{27BF}' | '\u{1F000}'..='\u{1FAFF}')
}

/// Where `text` has code, inline or in blocks, as byte ranges.
fn code(text: &str) -> Vec<Range<usize>> {
    render::events(text)
        .filter_map(|(event, range)| match event {
            Event::Code(_) | Event::InlineMath(_) | Event::DisplayMath(_) | Event::Start(Tag::CodeBlock(_)) => Some(range),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found<'t>(text: &'t str, ranges: &[Range<usize>]) -> Vec<&'t str> {
        ranges.iter().map(|range| &text[range.clone()]).collect()
    }

    #[test]
    fn tags_take_letters_of_any_script_digits_and_nesting() {
        let text = "#tag and #nested/tag, #کتاب_ها #y1984\n#1984 is a year";
        assert_eq!(found(text, &tags(text)), vec!["#tag", "#nested/tag", "#کتاب_ها", "#y1984"]);
    }

    fn callout_of(kind: &'static str, title: &str, fold: Option<bool>) -> Option<Callout> {
        Some(Callout { kind, title: title.to_owned(), fold })
    }

    #[test]
    fn a_callouts_first_line_gives_its_kind_title_and_folding() {
        assert_eq!(callout("[!tip] Read this"), callout_of("tip", "Read this", None));
        assert_eq!(callout("[!hint]"), callout_of("tip", "Hint", None));
        assert_eq!(callout("[!FAQ]- Why?"), callout_of("question", "Why?", Some(false)));
        assert_eq!(callout("[!todo]+"), callout_of("todo", "Todo", Some(true)));
        assert_eq!(callout("[!caution] Hot"), callout_of("warning", "Hot", None));
        assert_eq!(callout("[!WARNING]"), callout_of("warning", "Warning", None));
    }

    #[test]
    fn unknown_kinds_look_like_notes_and_plain_quotes_are_not_callouts() {
        assert_eq!(callout("[!recipe|wide] Pasta"), callout_of("note", "Pasta", None));
        assert_eq!(callout("[!my-type]"), callout_of("note", "My-type", None));
        assert_eq!(callout("Just a quote"), None);
        assert_eq!(callout("[!] nothing"), None);
        assert_eq!(callout("[!two words] no"), None);
    }

    #[test]
    fn highlights_stay_on_one_line_and_have_something_in_them() {
        let text = "a ==marked== b ==== c ==two\nlines== `==code==` ==Persian نشان==";
        assert_eq!(found(text, &additions(text).highlights), vec!["==marked==", "==Persian نشان=="]);
    }

    #[test]
    fn comments_run_to_the_next_double_percent_or_the_end() {
        let text = "keep %%hidden%% keep\n%%\nblock\n\nstill\n%% after `%%code%%` \\%%escaped %%open to the end";
        assert_eq!(found(text, &additions(text).comments), vec!["%%hidden%%", "%%\nblock\n\nstill\n%%", "%%open to the end"]);
    }

    #[test]
    fn block_ids_end_a_line_after_a_space_or_stand_alone() {
        let text = "A paragraph ^para-1\n- item ^item_2\n^standalone\nnot^this\n`code ^no`\nend ^x y";
        assert_eq!(found(text, &additions(text).block_ids), vec!["^para-1", "^item_2", "^standalone"]);
    }

    #[test]
    fn properties_read_text_lists_and_quoted_links() {
        let yaml = "title: Trip plan\ntags:\n  - travel\n  - sicily\naliases: [Plan, \"Sicily trip\"]\nrelated: \"[[Plans]]\"\ntime: 10:30\nempty:\n";
        let owned = |list: &[&str]| list.iter().map(|item| item.to_string()).collect::<Vec<_>>();
        assert_eq!(
            properties(yaml),
            vec![
                ("title".to_owned(), owned(&["Trip plan"])),
                ("tags".to_owned(), owned(&["travel", "sicily"])),
                ("aliases".to_owned(), owned(&["Plan", "Sicily trip"])),
                ("related".to_owned(), owned(&["[[Plans]]"])),
                ("time".to_owned(), owned(&["10:30"])),
                ("empty".to_owned(), owned(&[])),
            ]
        );
    }

    #[test]
    fn a_notes_aliases_come_from_its_front_matter() {
        assert_eq!(aliases("---\ntitle: x\naliases: [Sicily trip, \"Trip 2026\"]\n---\n# Plan\n"), vec!["Sicily trip", "Trip 2026"]);
        assert_eq!(aliases("---\nalias: Only one\n---\n"), vec!["Only one"]);
        assert_eq!(aliases("---\naliases:\n  - First\n  - Second\n---\n"), vec!["First", "Second"]);
        assert_eq!(aliases("# Not front matter\naliases: [Nope]\n"), Vec::<String>::new());
    }

    #[test]
    fn a_block_id_names_its_paragraph_its_list_item_or_the_block_above() {
        let text = "Intro\n\nFirst line\nsecond line ^para\n\n- one\n- two ^item\n- three\n\n| a |\n|---|\n\n^table\n\n`^code` here\n";
        let found = |id| block(text, id).map(|range| &text[range]);
        assert_eq!(found("para"), Some("First line\nsecond line ^para"));
        assert_eq!(found("item"), Some("- two ^item"));
        assert_eq!(found("table"), Some("| a |\n|---|"));
        assert_eq!(found("code"), None);
        assert_eq!(found("nope"), None);
    }

    #[test]
    fn headings_code_links_and_words_with_a_hash_inside_are_not_tags() {
        let text = "# Heading\n## Two\na#b x/#y [[Note#heading]] `#code`\n```\n#block\n```\n#last/";
        assert_eq!(found(text, &tags(text)), vec!["#last"]);
    }
}

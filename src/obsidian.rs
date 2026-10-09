//! Obsidian's additions to Markdown, as its help describes them: tags, and (to come)
//! highlights, comments, callouts and block ids.

use std::ops::Range;

use pulldown_cmark::{Event, Tag};

use crate::render;

/// The tags in `text` (`#tag`, `#nested/tag`, `#کتاب`), as byte ranges with their `#`: after
/// the start of a line or a space, letters, digits, `_`, `-` and `/`, at least one of them
/// not a digit; never in code.
pub fn tags(text: &str) -> Vec<Range<usize>> {
    let code = code(text);
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
            Event::Code(_) | Event::Start(Tag::CodeBlock(_)) => Some(range),
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

    #[test]
    fn headings_code_links_and_words_with_a_hash_inside_are_not_tags() {
        let text = "# Heading\n## Two\na#b x/#y [[Note#heading]] `#code`\n```\n#block\n```\n#last/";
        assert_eq!(found(text, &tags(text)), vec!["#last"]);
    }
}

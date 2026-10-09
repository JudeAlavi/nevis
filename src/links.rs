//! Where a link inside a note points: the file to open and the heading to scroll to.

use pulldown_cmark::{Event, LinkType, Tag};

/// A link's file part (relative to the note; empty means the note itself) and heading.
#[derive(Debug, PartialEq)]
pub struct Target {
    pub path: String,
    pub heading: Option<String>,
}

/// Whether a link names its own scheme (https:, mailto:…): it goes to the web or another app,
/// not to a note.
pub fn has_scheme(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() > 1 && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    })
}

/// Whether `text` is a web (or mail) address on its own, to link words to.
pub fn is_web_address(text: &str) -> bool {
    has_scheme(text) && !text.contains(char::is_whitespace) && (text.contains("://") || text.starts_with("mailto:"))
}

/// Splits a relative link or a `[[wiki]]` name: "Note#Heading" opens Note.md at Heading.
pub fn split(link: &str) -> Target {
    let (path, heading) = match link.split_once('#') {
        Some((path, heading)) => (path, Some(decode(heading))),
        None => (link, None),
    };
    let mut path = decode(path);
    let name = path.rsplit('/').next().unwrap_or_default();
    if !path.is_empty() && !name.contains('.') {
        path.push_str(".md");
    }
    Target { path, heading }
}

/// Undoes %XX escapes ("My%20Note" → "My Note"), multi-byte characters included; anything
/// that isn't a valid escape is kept as it is.
pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                i += 3;
            }
            (byte, _, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// The link at byte `at` in Markdown `source` (on its words, its address or its brackets),
/// as it would be followed: what a click there opens. Pictures and embedded notes aren't
/// followed.
pub fn at(source: &str, at: usize) -> Option<String> {
    for (event, range) in crate::render::events(source) {
        match event {
            Event::Start(Tag::Link { link_type, dest_url, .. }) if range.contains(&at) => {
                return Some(match link_type {
                    LinkType::Email => format!("mailto:{dest_url}"),
                    _ => dest_url.to_string(),
                });
            }
            Event::Start(Tag::Image { .. }) if range.contains(&at) => return None,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_under_a_click_is_what_it_opens() {
        let text = "See [the plan](Plans.md#Budget) and [[Notes/Trip|trip]] or <https://x.org>.\n![[pic.png]] [[Other]]";
        let under = |words: &str| at(text, text.find(words).unwrap());
        assert_eq!(under("plan"), Some("Plans.md#Budget".to_owned()));
        assert_eq!(under("(Plans"), Some("Plans.md#Budget".to_owned()));
        assert_eq!(under("trip]]"), Some("Notes/Trip".to_owned()));
        assert_eq!(under("x.org"), Some("https://x.org".to_owned()));
        assert_eq!(under("Other"), Some("Other".to_owned()));
        assert_eq!(under("See"), None);
        assert_eq!(under("pic.png"), None);
    }

    fn target(path: &str, heading: Option<&str>) -> Target {
        Target { path: path.to_owned(), heading: heading.map(str::to_owned) }
    }

    #[test]
    fn a_wiki_name_gains_md() {
        assert_eq!(split("Note"), target("Note.md", None));
    }

    #[test]
    fn the_part_after_a_hash_is_the_heading() {
        assert_eq!(split("Note#Heading"), target("Note.md", Some("Heading")));
        assert_eq!(split("#Heading"), target("", Some("Heading")));
    }

    #[test]
    fn percent_encoded_links_decode_persian_included() {
        assert_eq!(split("folder/My%20Note.md#x%20y"), target("folder/My Note.md", Some("x y")));
        assert_eq!(split("%DB%8C%D8%A7%D8%AF%D8%AF%D8%A7%D8%B4%D8%AA"), target("یادداشت.md", None));
    }

    #[test]
    fn existing_extensions_stay_and_folder_dots_do_not_count() {
        assert_eq!(split("todo.txt"), target("todo.txt", None));
        assert_eq!(split("a/b.md"), target("a/b.md", None));
        assert_eq!(split("v1.2/notes"), target("v1.2/notes.md", None));
    }
}

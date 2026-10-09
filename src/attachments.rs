//! Pasted images: saved as PNG files in `attachments/` beside the note, and linked from it.
//! A note with no file yet keeps them aside until its first save brings them beside it.

use std::path::PathBuf;

use crate::{links, positions};

/// Where images pasted into a note with no file yet wait for its first save.
pub fn aside() -> PathBuf {
    positions::state_dir().join("attachments")
}

/// How a link to an image kept `aside` starts, as `link` writes it, and how it starts once the
/// image is beside the note.
pub fn starts(aside: &str) -> (String, &'static str) {
    (format!("]({}/", encode(aside)), "](attachments/")
}

/// A pasted image's file name, from when it was pasted: "Pasted image 2026-10-08 16-20-11.png",
/// numbered when `taken` says that name is in use.
pub fn name(when: (i32, u32, u32, u32, u32, u32), taken: impl Fn(&str) -> bool) -> String {
    let (year, month, day, hour, minute, second) = when;
    let stem = format!("Pasted image {year:04}-{month:02}-{day:02} {hour:02}-{minute:02}-{second:02}");
    (1..)
        .map(|n| if n == 1 { format!("{stem}.png") } else { format!("{stem} {n}.png") })
        .find(|name| !taken(name))
        .unwrap_or_default()
}

/// The Markdown for the image at `path` (relative to the note, or absolute), with spaces and
/// the like percent-encoded, as Obsidian and GitHub write and read it.
pub fn link(path: &str) -> String {
    format!("![]({})", encode(path))
}

fn encode(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            ' ' => encoded.push_str("%20"),
            '%' => encoded.push_str("%25"),
            '(' => encoded.push_str("%28"),
            ')' => encoded.push_str("%29"),
            '<' => encoded.push_str("%3C"),
            '>' => encoded.push_str("%3E"),
            c => encoded.push(c),
        }
    }
    encoded
}

/// `text` with the images kept `aside` (an absolute folder) while the note had no file linked
/// in `attachments/` beside it instead, and the names of those images, to move there.
pub fn settle(text: &str, aside: &str) -> (String, Vec<String>) {
    // As `link` wrote them: "](" and the encoded path, which has no ")" of its own.
    let (start, settled_start) = starts(aside);
    let (mut settled, mut moved) = (String::with_capacity(text.len()), Vec::new());
    let mut rest = text;
    while let Some(at) = rest.find(&start) {
        let after = &rest[at + start.len()..];
        let Some(end) = after.find(')') else { break };
        settled.push_str(&rest[..at]);
        settled.push_str(settled_start);
        settled.push_str(&after[..end]);
        settled.push(')');
        moved.push(links::decode(&after[..end]));
        rest = &after[end + 1..];
    }
    settled.push_str(rest);
    (settled, moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHEN: (i32, u32, u32, u32, u32, u32) = (2026, 10, 8, 16, 20, 11);

    #[test]
    fn a_pasted_image_is_named_by_when_and_numbered_if_taken() {
        assert_eq!(name(WHEN, |_| false), "Pasted image 2026-10-08 16-20-11.png");
        assert_eq!(name(WHEN, |name| name == "Pasted image 2026-10-08 16-20-11.png"), "Pasted image 2026-10-08 16-20-11 2.png");
    }

    #[test]
    fn links_percent_encode_what_would_break_them_and_keep_other_letters() {
        assert_eq!(link("attachments/Pasted image 2026-10-08 16-20-11.png"), "![](attachments/Pasted%20image%202026-10-08%2016-20-11.png)");
        assert_eq!(link("a (copy) 100%.png"), "![](a%20%28copy%29%20100%25.png)");
        assert_eq!(link("پیوست‌ها/عکس.png"), "![](پیوست‌ها/عکس.png)");
    }

    #[test]
    fn images_kept_aside_move_beside_the_note_on_its_first_save() {
        let aside = "/home/me/.local/state/nevis/attachments";
        let text = "Before\n![](/home/me/.local/state/nevis/attachments/Pasted%20image%201.png)\n![](other.png) and ![](/elsewhere/x.png)\n";
        let (settled, moved) = settle(text, aside);
        assert_eq!(settled, "Before\n![](attachments/Pasted%20image%201.png)\n![](other.png) and ![](/elsewhere/x.png)\n");
        assert_eq!(moved, vec!["Pasted image 1.png".to_owned()]);
    }
}

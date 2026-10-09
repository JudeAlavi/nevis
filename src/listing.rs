//! Which entries the folder sidebar shows, and in what order.

use std::cmp::Ordering;

/// Markdown files: notes.
const MARKDOWN: [&str; 5] = ["md", "markdown", "mdown", "mkd", "mkdn"];
/// The other files nevis opens from the sidebar.
const PLAIN: [&str; 2] = ["txt", "text"];

/// Whether the sidebar lists this entry.
pub fn shown(name: &str, is_dir: bool, is_hidden: bool) -> bool {
    if is_hidden || name.starts_with('.') {
        return false;
    }
    if is_dir {
        return true;
    }
    is_note(name) || has_extension(name, &PLAIN)
}

/// Whether a file is a Markdown note, by its name.
pub fn is_note(name: &str) -> bool {
    has_extension(name, &MARKDOWN)
}

fn has_extension(name: &str, extensions: &[&str]) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extensions.iter().any(|known| extension.eq_ignore_ascii_case(known)))
}

/// Sidebar order of two entries, each (name, is_dir): folders first, then by name.
pub fn order(a: (&str, bool), b: (&str, bool)) -> Ordering {
    b.1.cmp(&a.1).then_with(|| natural(a.0, b.0)).then_with(|| a.0.cmp(b.0))
}

/// Compares names with runs of digits taken as numbers: "note2" before "note10".
fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let by = if digit(x).is_some() && digit(y).is_some() {
                    number(&mut a).cmp(&number(&mut b))
                } else {
                    a.next();
                    b.next();
                    x.to_lowercase().cmp(y.to_lowercase())
                };
                if by != Ordering::Equal {
                    return by;
                }
            }
        }
    }
}

/// The value of a Latin, Persian (۰–۹) or Arabic-Indic (٠–٩) digit.
fn digit(c: char) -> Option<u32> {
    match c {
        '0'..='9' => c.to_digit(10),
        '\u{06F0}'..='\u{06F9}' => Some(c as u32 - 0x06F0),
        '\u{0660}'..='\u{0669}' => Some(c as u32 - 0x0660),
        _ => None,
    }
}

/// Consumes a run of digits and returns its value.
fn number(chars: &mut std::iter::Peekable<std::str::Chars>) -> u128 {
    let mut value: u128 = 0;
    while let Some(d) = chars.peek().copied().and_then(digit) {
        value = value.saturating_mul(10).saturating_add(d as u128);
        chars.next();
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_folders_and_text_files_only() {
        assert!(shown("notes", true, false));
        assert!(shown("a.md", false, false));
        assert!(shown("b.TXT", false, false));
        assert!(!shown("photo.png", false, false));
        assert!(!shown("Makefile", false, false));
    }

    #[test]
    fn hides_hidden_entries() {
        assert!(!shown(".git", true, false));
        assert!(!shown(".draft.md", false, false));
        assert!(!shown("backup.md", false, true));
        assert!(!shown("archive", true, true));
    }

    #[test]
    fn folders_come_before_files() {
        assert_eq!(order(("zeta", true), ("alpha.md", false)), Ordering::Less);
        assert_eq!(order(("alpha.md", false), ("zeta", true)), Ordering::Greater);
    }

    #[test]
    fn numbers_sort_by_value() {
        assert_eq!(order(("note2.md", false), ("note10.md", false)), Ordering::Less);
        assert_eq!(order(("week 9", true), ("week 10", true)), Ordering::Less);
    }

    #[test]
    fn case_does_not_split_the_alphabet() {
        assert_eq!(order(("apple.md", false), ("Banana.md", false)), Ordering::Less);
        assert_eq!(order(("Cherry.md", false), ("banana.md", false)), Ordering::Greater);
    }

    #[test]
    fn persian_and_arabic_digits_sort_by_value_too() {
        assert_eq!(order(("یادداشت ۲.md", false), ("یادداشت ۱۰.md", false)), Ordering::Less);
        assert_eq!(order(("درس ٣", true), ("درس ١٢", true)), Ordering::Less);
    }
}

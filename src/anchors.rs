//! Links to headings: `[text](#fast-notes)` (GitHub style) and `[[#Fast notes]]` (Obsidian).

/// A heading's GitHub-style anchor: lowercase, punctuation dropped, spaces as hyphens.
pub fn slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            _ if c.is_whitespace() => Some('-'),
            // The Persian half-space is part of the word ("زبان‌ها"), not punctuation.
            '-' | '_' | '\u{200c}' => Some(c),
            _ if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .collect()
}

/// Whether a link target (what follows `#`) points at `heading`: as its slug, or as its text
/// the way Obsidian writes `[[#Heading]]`, case aside.
pub fn points_at(target: &str, heading: &str) -> bool {
    slug(target) == slug(heading)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_lowercase_and_hyphenate() {
        assert_eq!(slug("Fast notes"), "fast-notes");
    }

    #[test]
    fn slugs_drop_punctuation_but_keep_hyphens_and_underscores() {
        assert_eq!(slug("What's new? (v2_beta)"), "whats-new-v2_beta");
        assert_eq!(slug("Fast notes — tips"), "fast-notes--tips");
    }

    #[test]
    fn persian_headings_keep_their_letters_and_half_spaces() {
        assert_eq!(slug("فهرست کارها"), "فهرست-کارها");
        assert_eq!(slug("تست زبان\u{200c}ها"), "تست-زبان\u{200c}ها");
    }

    #[test]
    fn a_link_finds_its_heading_written_either_way() {
        assert!(points_at("fast-notes", "Fast notes"));
        assert!(points_at("Fast notes", "Fast notes"));
        assert!(points_at("fast notes", "Fast Notes"));
        assert!(points_at("فهرست-کارها", "فهرست کارها"));
        assert!(!points_at("other", "Fast notes"));
    }
}

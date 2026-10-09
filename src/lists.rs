//! Markdown lists continue as you type: Enter after "- item" starts "- ", after "3. item"
//! starts "4. ", and Enter on an empty item ends the list.

#[derive(Debug, PartialEq)]
pub enum Enter {
    /// Start the new line with this.
    Continue(String),
    /// The item was empty: the line becomes this instead (a level out, or nothing).
    Replace(String),
}

/// What Enter does on a line, given the text before the cursor and after it; nothing special
/// when the line isn't a list item or a quote.
pub fn on_enter(before: &str, after: &str) -> Option<Enter> {
    let start = Start::read(before)?;
    if !start.text.trim().is_empty() || !after.trim().is_empty() {
        return Some(Enter::Continue(start.next()));
    }
    // An empty item: out one level when nested, else out of the list (a quote stays).
    let line = match &start.item {
        Some(item) if !start.indent.is_empty() => {
            let task = if item.task { "[ ] " } else { "" };
            format!("{}{}{} {task}", outdent(start.indent), start.quote, item.marker)
        }
        Some(_) => start.quote.to_owned(),
        None => String::new(),
    };
    Some(Enter::Replace(line))
}

/// `indent` one level less: a tab or up to four spaces fewer.
fn outdent(indent: &str) -> &str {
    match indent.strip_prefix('\t') {
        Some(rest) => rest,
        None => &indent[indent.len().min(4)..],
    }
}

/// How a line starts: its indent, its quote marks and its list marker.
struct Start<'a> {
    indent: &'a str,
    quote: &'a str,
    item: Option<Item>,
    /// What the line says after all that.
    text: &'a str,
}

/// A list item's marker.
struct Item {
    /// As written: "-", "3.".
    marker: String,
    /// What the next item gets: the same bullet, or the next number.
    next: String,
    /// It has a checkbox.
    task: bool,
}

impl<'a> Start<'a> {
    /// The start of a quote or list line; nothing for other lines.
    fn read(line: &'a str) -> Option<Self> {
        let indent = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];
        let rest = &line[indent.len()..];
        // Quote marks: each ">" maybe followed by a space.
        let bytes = rest.as_bytes();
        let mut quoted = 0;
        while bytes.get(quoted) == Some(&b'>') {
            quoted += 1;
            if bytes.get(quoted) == Some(&b' ') {
                quoted += 1;
            }
        }
        let (quote, rest) = rest.split_at(quoted);
        let (item, text) = match Item::read(rest) {
            Some((item, text)) => (Some(item), text),
            None => (None, rest),
        };
        (!quote.is_empty() || item.is_some()).then_some(Start { indent, quote, item, text })
    }

    /// What the next line starts with.
    fn next(&self) -> String {
        match &self.item {
            Some(item) => {
                let task = if item.task { "[ ] " } else { "" };
                format!("{}{}{} {task}", self.indent, self.quote, item.next)
            }
            None => format!("{}{}", self.indent, self.quote),
        }
    }
}

impl Item {
    /// The marker at the start of `text` ("- ", "* ", "+ ", "3. ", "3) ", maybe with a
    /// box), and the text after it.
    fn read(text: &str) -> Option<(Self, &str)> {
        let (marker, next, after) = match text.get(..1) {
            Some(bullet @ ("-" | "*" | "+")) => (bullet.to_owned(), bullet.to_owned(), &text[1..]),
            _ => {
                let digits = text.bytes().take_while(u8::is_ascii_digit).count();
                let delimiter = text[digits..].chars().next().filter(|c| matches!(c, '.' | ')'))?;
                let number: u64 = text[..digits.min(9)].parse().ok().filter(|_| digits <= 9)?;
                (text[..=digits].to_owned(), format!("{}{delimiter}", number + 1), &text[digits + 1..])
            }
        };
        let rest = after.strip_prefix([' ', '\t'])?;
        let task = matches!(rest.get(..3), Some("[ ]" | "[x]" | "[X]"))
            && rest[3..].chars().next().is_none_or(|c| c == ' ' || c == '\t');
        let text = if task { rest[3..].trim_start_matches([' ', '\t']) } else { rest };
        Some((Item { marker, next, task }, text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn continues(before: &str) -> Option<Enter> {
        on_enter(before, "")
    }

    fn with(prefix: &str) -> Option<Enter> {
        Some(Enter::Continue(prefix.to_owned()))
    }

    #[test]
    fn bullets_continue_with_their_marker_and_indent() {
        assert_eq!(continues("- milk"), with("- "));
        assert_eq!(continues("* eggs"), with("* "));
        assert_eq!(continues("+ bread"), with("+ "));
        assert_eq!(continues("    - nested"), with("    - "));
        // Not list items.
        assert_eq!(continues("plain text"), None);
        assert_eq!(continues("-no space"), None);
        assert_eq!(continues("**bold** start"), None);
    }

    #[test]
    fn numbers_count_up_and_tasks_start_unticked() {
        assert_eq!(continues("1. first"), with("2. "));
        assert_eq!(continues("  9) ninth"), with("  10) "));
        assert_eq!(continues("1.5 kg of flour"), None);
        assert_eq!(continues("- [x] done"), with("- [ ] "));
        assert_eq!(continues("3. [ ] todo"), with("4. [ ] "));
    }

    #[test]
    fn quotes_continue_and_so_do_lists_inside_them() {
        assert_eq!(continues("> wise words"), with("> "));
        assert_eq!(continues("> > deeper"), with("> > "));
        assert_eq!(continues(">no space"), with(">"));
        assert_eq!(continues("> - quoted item"), with("> - "));
    }

    fn replaced(line: &str) -> Option<Enter> {
        Some(Enter::Replace(line.to_owned()))
    }

    #[test]
    fn an_empty_item_ends_the_list_or_steps_out_a_level() {
        assert_eq!(on_enter("- ", ""), replaced(""));
        assert_eq!(on_enter("3. [ ] ", ""), replaced(""));
        assert_eq!(on_enter("    - ", ""), replaced("- "));
        assert_eq!(on_enter("\t\t2. [ ] ", ""), replaced("\t2. [ ] "));
        // In a quote, an empty item leaves the list but not the quote; an empty quote ends.
        assert_eq!(on_enter("> - ", ""), replaced("> "));
        assert_eq!(on_enter("> ", " "), replaced(""));
        // Text after the cursor: the item isn't empty, it moves down as a new item.
        assert_eq!(on_enter("- ", "milk"), with("- "));
    }
}

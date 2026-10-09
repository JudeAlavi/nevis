//! Names for new and renamed notes.

/// The name suggested for a new note in a folder whose entries are `taken`.
pub fn new_note(taken: &[String]) -> String {
    (1..)
        .map(|n| if n == 1 { "Untitled.md".to_owned() } else { format!("Untitled {n}.md") })
        .find(|name| !taken.contains(name))
        .expect("a free name")
}

/// The file name for what the user typed: trimmed, with ".md" added when it has no
/// extension; or why it can't be used.
pub fn chosen(input: &str) -> Result<String, &'static str> {
    let name = chosen_folder(input)?;
    if name.contains('.') { Ok(name) } else { Ok(format!("{name}.md")) }
}

/// What a note with no file is called: its first line of words (a heading without its #),
/// at most 40 characters; lines holding only an image don't count.
pub fn of_text(text: &str) -> String {
    let image = |line: &str| (line.starts_with("![") && line.ends_with(')')) || (line.starts_with("![[") && line.ends_with("]]"));
    let line = text
        .lines()
        .map(|line| line.trim_start_matches('#').trim())
        .find(|line| !line.is_empty() && !image(line))
        .unwrap_or("");
    match line.chars().count() {
        0 => "Untitled".to_owned(),
        n if n > 40 => format!("{}…", line.chars().take(40).collect::<String>()),
        _ => line.to_owned(),
    }
}

/// A folder's name for what the user typed: trimmed; or why it can't be used.
pub fn chosen_folder(input: &str) -> Result<String, &'static str> {
    let name = input.trim();
    if name.is_empty() {
        return Err("Type a name.");
    }
    if name.contains('/') {
        return Err("A name can't contain “/”.");
    }
    if name.starts_with('.') {
        return Err("Names starting with “.” are hidden.");
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_new_note_takes_the_first_free_untitled_name() {
        assert_eq!(new_note(&[]), "Untitled.md");
        assert_eq!(new_note(&names(&["Untitled.md"])), "Untitled 2.md");
        assert_eq!(new_note(&names(&["Untitled.md", "Untitled 2.md", "notes.md"])), "Untitled 3.md");
    }

    #[test]
    fn a_typed_name_is_trimmed_and_gains_md_unless_it_has_an_extension() {
        assert_eq!(chosen("  Ideas  "), Ok("Ideas.md".to_owned()));
        assert_eq!(chosen("todo.txt"), Ok("todo.txt".to_owned()));
        assert_eq!(chosen("یادداشت تازه"), Ok("یادداشت تازه.md".to_owned()));
    }

    #[test]
    fn unusable_names_are_refused_with_a_reason() {
        assert!(chosen("").is_err());
        assert!(chosen("   ").is_err());
        assert!(chosen("plans/2026").is_err());
        assert!(chosen(".hidden").is_err());
        assert!(chosen("..").is_err());
    }

    #[test]
    fn a_note_without_a_file_is_called_by_its_first_line_of_words() {
        assert_eq!(of_text("\n## Shopping list\n- milk\n"), "Shopping list");
        assert_eq!(of_text("![](/home/me/.local/state/nevis/attachments/Pasted%20image.png)\n\nTrip photos\n"), "Trip photos");
        assert_eq!(of_text("![[sketch.png]]\nفهرست خرید\n"), "فهرست خرید");
        assert_eq!(of_text("   \n"), "Untitled");
        assert_eq!(of_text(&"x".repeat(50)), format!("{}…", "x".repeat(40)));
    }

    #[test]
    fn a_folder_name_follows_the_same_rules_without_gaining_md() {
        assert_eq!(chosen_folder("  Archive "), Ok("Archive".to_owned()));
        assert_eq!(chosen_folder("درس‌ها"), Ok("درس‌ها".to_owned()));
        assert!(chosen_folder("a/b").is_err());
        assert!(chosen_folder(".cache").is_err());
        assert!(chosen_folder(" ").is_err());
    }
}

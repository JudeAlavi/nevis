//! Read mode: Markdown drawn as formatted text, with the markup itself hidden.

use std::collections::HashMap;
use std::ops::Range;

use gtk::{gdk, gio, glib, pango, prelude::*};
use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};
use sourceview::prelude::*;

use crate::{flowchart, links, math};
use crate::theme::{self, fonts};

/// What a click on the rendered text does.
#[derive(Clone)]
pub enum Target {
    /// A URL, a path relative to the file, or a `[[wiki]]` page name.
    Link(String),
    /// A task checkbox: the byte range of its `[ ]` / `[x]` in the Markdown source.
    Task(Range<usize>),
    /// A note, by its file (the notes linking here, under the rendered note).
    File(gio::File),
}

/// Markdown features nevis understands, shared by the read view and the source styling.
pub const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_FOOTNOTES)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_GFM)
    .union(Options::ENABLE_WIKILINKS)
    .union(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS)
    .union(Options::ENABLE_MATH);

/// The Markdown's events, with where each is in `source`. Front matter counts only at the very
/// top of a note, as in Obsidian: pulldown-cmark takes any `---` line after a blank one, with
/// text right under it, for the start of some, and the rest of the note goes with it.
pub fn events(source: &str) -> impl Iterator<Item = (Event<'_>, Range<usize>)> {
    let top = front_matter(source);
    let rest = OPTIONS.difference(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    Parser::new_ext(&source[..top], OPTIONS).into_offset_iter().chain(
        Parser::new_ext(&source[top..], rest)
            .into_offset_iter()
            .map(move |(event, range)| (event, range.start + top..range.end + top)),
    )
}

/// How long the note's front matter is, in bytes: from a first line of `---` through the `---`
/// or `...` line that closes it; 0 when the note has none.
fn front_matter(source: &str) -> usize {
    let mut lines = source.split_inclusive('\n');
    let Some(first) = lines.next().filter(|line| line.trim_end() == "---") else { return 0 };
    let mut length = first.len();
    for line in lines {
        length += line.len();
        if matches!(line.trim_end(), "---" | "...") {
            return length;
        }
    }
    0
}

/// Side margins of the read view at full width (centred text has wider ones). Tag margins
/// replace the view's, so indents add to the margin.
pub const MARGIN: i32 = 28;

pub struct Rendered {
    pub buffer: sourceview::Buffer,
    /// Clickable spans; each has its own tag.
    pub targets: Vec<(gtk::TextTag, Target)>,
    /// Images and tables, to attach once `buffer` is on a view.
    pub children: Vec<(gtk::TextChildAnchor, gtk::Widget)>,
    /// Each heading's text and where it starts, for `#heading` links.
    pub headings: Vec<(String, gtk::TextMark)>,
    /// Fenced code blocks that name a language, to colour with `highlight_code`.
    pub code: Vec<CodeBlock>,
}

pub struct CodeBlock {
    pub start: gtk::TextMark,
    pub end: gtk::TextMark,
    pub language: String,
}

/// Renders `source` into a new buffer. `folder` resolves relative image paths; `find` finds the
/// file a link or embed names (in the vault, as Obsidian does), if there is one; `text` lays a
/// text out in the view's font (list bullets hang by their width; diagrams use it too);
/// `margin` is the view's side margin, which indented blocks add to.
pub fn render(
    source: &str,
    folder: Option<&gio::File>,
    find: &dyn Fn(&str) -> Option<gio::File>,
    scheme: Option<&sourceview::StyleScheme>,
    text: &dyn Fn(&str) -> pango::Layout,
    margin: i32,
) -> Rendered {
    let buffer = sourceview::Buffer::new(None);
    buffer.set_style_scheme(scheme);
    buffer.set_highlight_syntax(false);
    let mut r = Renderer::new(folder, find, buffer, text, margin);
    for (event, range) in events(source) {
        r.event(event, range);
    }
    r.finish()
}

struct Styles {
    headings: [gtk::TextTag; 6],
    paragraph: gtk::TextTag,
    strong: gtk::TextTag,
    emphasis: gtk::TextTag,
    strike: gtk::TextTag,
    code: gtk::TextTag,
    code_block: gtk::TextTag,
    code_top: gtk::TextTag,
    code_bottom: gtk::TextTag,
    quote: gtk::TextTag,
    callout: gtk::TextTag,
    done: gtk::TextTag,
    dim: gtk::TextTag,
    rule: gtk::TextTag,
    gap: gtk::TextTag,
    /// A diagram's line: centred.
    figure: gtk::TextTag,
    math: gtk::TextTag,
    display_math: gtk::TextTag,
}

impl Styles {
    fn new(table: &gtk::TextTagTable, margin: i32) -> Self {
        let add = |tag: gtk::TextTag| {
            table.add(&tag);
            tag
        };
        let heading = |scale: f64, above: i32| {
            add(gtk::TextTag::builder()
                .weight(700)
                .scale(scale)
                .foreground(theme::colors().heading.to_hex())
                .pixels_above_lines(above)
                .pixels_below_lines(6)
                .build())
        };
        Self {
            headings: [heading(1.8, 18), heading(1.5, 16), heading(1.3, 12), heading(1.15, 10), heading(1.05, 8), heading(1.0, 8)],
            paragraph: add(gtk::TextTag::builder().pixels_below_lines(10).build()),
            strong: add(gtk::TextTag::builder().weight(700).build()),
            emphasis: add(gtk::TextTag::builder().style(pango::Style::Italic).build()),
            strike: add(gtk::TextTag::builder().strikethrough(true).build()),
            code: add(gtk::TextTag::builder().family(fonts::MONO).foreground(theme::colors().code.to_hex()).background(theme::colors().code_background.to_hex()).build()),
            code_block: add(gtk::TextTag::builder()
                .family(fonts::MONO)
                .foreground(theme::colors().code.to_hex())
                .paragraph_background(theme::colors().code_background.to_hex())
                .direction(gtk::TextDirection::Ltr)
                .left_margin(margin + 12)
                .right_margin(margin + 12)
                .build()),
            code_top: add(gtk::TextTag::builder().pixels_above_lines(8).build()),
            code_bottom: add(gtk::TextTag::builder().pixels_below_lines(14).build()),
            quote: add(gtk::TextTag::builder()
                .paragraph_background(theme::colors().quote_background.to_hex())
                .foreground(theme::colors().quote.to_hex())
                .left_margin(margin + 14)
                .right_margin(margin + 14)
                .pixels_above_lines(2)
                .pixels_below_lines(2)
                .build()),
            callout: add(gtk::TextTag::builder().weight(700).foreground(theme::colors().accent.to_hex()).build()),
            done: add(gtk::TextTag::builder().strikethrough(true).foreground(theme::colors().dim.to_hex()).build()),
            dim: add(gtk::TextTag::builder().foreground(theme::colors().dim.to_hex()).build()),
            rule: add(gtk::TextTag::builder().size_points(1.0).paragraph_background(theme::colors().rule.to_hex()).build()),
            figure: add(gtk::TextTag::builder().justification(gtk::Justification::Center).pixels_above_lines(4).build()),
            math: add(gtk::TextTag::builder().family(fonts::MATH).build()),
            display_math: add(gtk::TextTag::builder()
                .justification(gtk::Justification::Center)
                .direction(gtk::TextDirection::Ltr)
                .scale(1.15)
                .pixels_above_lines(4)
                .pixels_below_lines(12)
                .build()),
            gap: add(gtk::TextTag::builder().size_points(5.0).build()),
        }
    }
}

struct List {
    /// Next number for ordered lists.
    next: Option<u64>,
}

/// A table being collected.
struct Table {
    aligns: Vec<Alignment>,
    rows: Vec<(bool, Vec<Cell>)>,
    head: bool,
}

#[derive(Default)]
struct Cell {
    markup: String,
    plain: String,
}

struct Renderer<'m> {
    folder: Option<gio::File>,
    /// The file a link or embed names, if there is one.
    find: &'m dyn Fn(&str) -> Option<gio::File>,
    text: &'m dyn Fn(&str) -> pango::Layout,
    /// The view's side margin, which indents add to.
    margin: i32,
    buffer: sourceview::Buffer,
    styles: Styles,
    /// Tags for everything inserted now: block-level ones, then inline ones.
    block: Vec<gtk::TextTag>,
    inline: Vec<gtk::TextTag>,
    lists: Vec<List>,
    quote_depth: usize,
    /// A list item started and its bullet is not written yet.
    pending_bullet: bool,
    /// Where each open list item's indent tag sits in `block`, to swap in its hanging indent.
    items: Vec<usize>,
    /// List indents by (depth, hanging width, right-to-left).
    indents: HashMap<(usize, i32, bool), gtk::TextTag>,
    code_start: Option<i32>,
    code_language: Option<String>,
    code: Vec<CodeBlock>,
    /// The heading being written: where it starts and its text so far.
    heading: Option<(i32, String)>,
    headings: Vec<(String, gtk::TextMark)>,
    /// Image URL and alt text, while inside `![…](…)`.
    image: Option<(String, String)>,
    table: Option<Table>,
    /// Closing Pango tags for inline styles inside a table cell.
    markup_close: Vec<&'static str>,
    targets: Vec<(gtk::TextTag, Target)>,
    children: Vec<(gtk::TextChildAnchor, gtk::Widget)>,
    /// Raised or lowered, smaller text for formulas' scripts, by rise and depth.
    scripts: HashMap<(i8, u8), gtk::TextTag>,
    /// The source of the Mermaid block being read, kept aside to draw at its end.
    mermaid: Option<String>,
}

impl<'m> Renderer<'m> {
    fn new(
        folder: Option<&gio::File>,
        find: &'m dyn Fn(&str) -> Option<gio::File>,
        buffer: sourceview::Buffer,
        text: &'m dyn Fn(&str) -> pango::Layout,
        margin: i32,
    ) -> Self {
        let styles = Styles::new(&buffer.tag_table(), margin);
        Self {
            folder: folder.cloned(),
            find,
            text,
            margin,
            buffer,
            styles,
            block: Vec::new(),
            inline: Vec::new(),
            lists: Vec::new(),
            quote_depth: 0,
            pending_bullet: false,
            items: Vec::new(),
            indents: HashMap::new(),
            code_start: None,
            code_language: None,
            code: Vec::new(),
            heading: None,
            headings: Vec::new(),
            image: None,
            table: None,
            markup_close: Vec::new(),
            targets: Vec::new(),
            children: Vec::new(),
            scripts: HashMap::new(),
            mermaid: None,
        }
    }

    /// `text`'s width in pixels in the view's font.
    fn measure(&self, text: &str) -> i32 {
        (self.text)(text).pixel_size().0
    }

    fn event(&mut self, event: Event, range: Range<usize>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            // U+2068…U+2069 isolate the span: `~/notes/a.md` inside Persian stays in order.
            Event::Code(text) => self.styled(&format!("\u{2068}{text}\u{2069}"), self.styles.code.clone(), "<tt>", "</tt>"),
            Event::InlineMath(latex) => self.math(&latex, false),
            Event::DisplayMath(latex) => self.math(&latex, true),
            Event::Html(html) | Event::InlineHtml(html) => {
                let tag = html.trim().to_ascii_lowercase();
                if matches!(tag.as_str(), "<br>" | "<br/>" | "<br />") {
                    self.text("\u{2028}");
                } else if !tag.starts_with("<!--") {
                    self.styled(&html, self.styles.dim.clone(), "", "");
                }
            }
            Event::FootnoteReference(label) => {
                self.styled(&format!("[{label}]"), self.styles.dim.clone(), "", "")
            }
            // Single line breaks stay line breaks, as in Obsidian. Each line is its own
            // paragraph in the view, so Persian and English lines each get their direction.
            Event::SoftBreak | Event::HardBreak => self.text("\n"),
            Event::Rule => {
                self.end_line();
                let mut end = self.buffer.end_iter();
                self.buffer.insert_with_tags(&mut end, " \n", &[&self.styles.rule]);
            }
            Event::TaskListMarker(done) => self.task(done, range),
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                if !self.pending_bullet {
                    self.end_line();
                }
            }
            Tag::Heading { level, .. } => {
                self.end_line();
                self.heading = Some((self.buffer.end_iter().offset(), String::new()));
                self.block.push(self.styles.headings[heading_index(level)].clone());
            }
            Tag::BlockQuote(kind) => {
                self.end_line();
                self.quote_depth += 1;
                self.block.push(self.styles.quote.clone());
                if let Some(kind) = kind {
                    let title = match kind {
                        BlockQuoteKind::Note => "Note",
                        BlockQuoteKind::Tip => "Tip",
                        BlockQuoteKind::Important => "Important",
                        BlockQuoteKind::Warning => "Warning",
                        BlockQuoteKind::Caution => "Caution",
                    };
                    self.styled(title, self.styles.callout.clone(), "", "");
                    self.end_line();
                }
            }
            Tag::CodeBlock(_) | Tag::MetadataBlock(_) => {
                self.end_line();
                if let Tag::CodeBlock(CodeBlockKind::Fenced(info)) = &tag {
                    self.code_language = info.split_whitespace().next().map(str::to_owned);
                }
                // A diagram is drawn once all of its source is in.
                if self.code_language.as_deref() == Some("mermaid") {
                    self.mermaid = Some(String::new());
                    return;
                }
                self.code_start = Some(self.buffer.end_iter().offset());
                self.block.push(self.styles.code_block.clone());
            }
            Tag::HtmlBlock => self.end_line(),
            Tag::List(first) => {
                if self.pending_bullet {
                    self.bullet();
                }
                self.lists.push(List { next: first });
            }
            Tag::Item => {
                self.end_line();
                let indent = self.indent(self.lists.len(), 0, false);
                self.items.push(self.block.len());
                self.block.push(indent);
                self.pending_bullet = true;
            }
            Tag::FootnoteDefinition(label) => {
                self.end_line();
                self.styled(&format!("[{label}] "), self.styles.dim.clone(), "", "");
            }
            Tag::Table(aligns) => {
                self.end_line();
                self.table = Some(Table { aligns, rows: Vec::new(), head: false });
            }
            Tag::TableHead => self.with_table(|t| {
                t.head = true;
                t.rows.push((true, Vec::new()));
            }),
            Tag::TableRow => self.with_table(|t| t.rows.push((false, Vec::new()))),
            Tag::TableCell => self.with_table(|t| {
                if let Some((_, cells)) = t.rows.last_mut() {
                    cells.push(Cell::default());
                }
            }),
            Tag::Emphasis => self.push_inline(self.styles.emphasis.clone(), "<i>", "</i>"),
            Tag::Strong => self.push_inline(self.styles.strong.clone(), "<b>", "</b>"),
            Tag::Strikethrough => self.push_inline(self.styles.strike.clone(), "<s>", "</s>"),
            Tag::Link { link_type, dest_url, .. } => {
                let url = match link_type {
                    LinkType::Email => format!("mailto:{dest_url}"),
                    _ => dest_url.to_string(),
                };
                // A link to a note that doesn't exist yet is fainter, as in Obsidian.
                let path = links::split(&url).path;
                let missing = !links::has_scheme(&url) && !path.is_empty() && (self.find)(&path).is_none();
                let link = theme::colors().link;
                let tag = gtk::TextTag::builder()
                    .foreground_rgba(&gdk::RGBA::new(link.r as f32, link.g as f32, link.b as f32, if missing { 0.55 } else { 1.0 }))
                    .underline(pango::Underline::Single)
                    .build();
                self.buffer.tag_table().add(&tag);
                self.targets.push((tag.clone(), Target::Link(url)));
                self.push_inline(tag, "<u>", "</u>");
                self.text("\u{2068}");
            }
            Tag::Image { dest_url, .. } => self.image = Some((dest_url.to_string(), String::new())),
            // Not enabled in the parser options.
            Tag::Superscript | Tag::Subscript | Tag::DefinitionList | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                if self.lists.is_empty() && self.table.is_none() {
                    self.space_last_line(&self.styles.paragraph.clone());
                }
                self.end_line();
            }
            TagEnd::Heading(_) => {
                self.end_line();
                self.block.pop();
                if let Some((start, text)) = self.heading.take() {
                    let mark = self.buffer.create_mark(None, &self.buffer.iter_at_offset(start), true);
                    self.headings.push((text, mark));
                }
            }
            TagEnd::BlockQuote(_) => {
                self.end_line();
                self.block.pop();
                self.quote_depth -= 1;
                if self.quote_depth == 0 {
                    self.gap();
                }
            }
            TagEnd::CodeBlock | TagEnd::MetadataBlock(_) => {
                // A flowchart is drawn; other diagrams are shown as their code.
                if let Some(source) = self.mermaid.take() {
                    if let Some(chart) = flowchart::parse(&source) {
                        self.code_language = None;
                        return self.figure(flowchart::picture(&chart, self.text).upcast());
                    }
                    self.code_start = Some(self.buffer.end_iter().offset());
                    self.block.push(self.styles.code_block.clone());
                    self.text(&source);
                }
                self.end_line();
                self.block.pop();
                if let Some(start) = self.code_start.take() {
                    if let Some(language) = self.code_language.take() {
                        let start = self.buffer.create_mark(None, &self.buffer.iter_at_offset(start), true);
                        let end = self.buffer.create_mark(None, &self.buffer.end_iter(), true);
                        self.code.push(CodeBlock { start, end, language });
                    }
                    self.space_code_block(start);
                }
                if self.quote_depth == 0 {
                    self.gap();
                }
            }
            TagEnd::HtmlBlock | TagEnd::FootnoteDefinition => self.end_line(),
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::Item => {
                if self.pending_bullet {
                    self.bullet();
                }
                self.end_line();
                self.block.pop();
                self.items.pop();
                // A checked task's strike-through ends with its item.
                if self.inline.last() == Some(&self.styles.done) {
                    self.inline.pop();
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    let grid = table_widget(table);
                    let mut end = self.buffer.end_iter();
                    let anchor = self.buffer.create_child_anchor(&mut end);
                    self.children.push((anchor, grid));
                    self.end_line();
                }
            }
            TagEnd::TableHead | TagEnd::TableRow | TagEnd::TableCell => {}
            TagEnd::Link => {
                self.text("\u{2069}");
                self.pop_inline();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_inline(),
            TagEnd::Image => {
                if let Some((url, alt)) = self.image.take() {
                    self.image(&url, &alt);
                }
            }
            TagEnd::Superscript | TagEnd::Subscript | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle | TagEnd::DefinitionListDefinition => {}
        }
    }

    /// Plain text in the current styles.
    fn text(&mut self, text: &str) {
        if let Some(source) = &mut self.mermaid {
            source.push_str(text);
            return;
        }
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(text);
            return;
        }
        if let Some(cell) = self.cell() {
            cell.markup.push_str(&glib::markup_escape_text(text));
            cell.plain.push_str(text);
            return;
        }
        if self.pending_bullet {
            self.bullet();
        }
        if let Some((_, heading)) = &mut self.heading {
            heading.push_str(text);
        }
        let tags: Vec<&gtk::TextTag> = self.block.iter().chain(self.inline.iter()).collect();
        let mut end = self.buffer.end_iter();
        self.buffer.insert_with_tags(&mut end, text, &tags);
    }

    /// Text with one extra style (or Pango markup inside a table cell).
    fn styled(&mut self, text: &str, tag: gtk::TextTag, open: &str, close: &str) {
        if let Some(cell) = self.cell() {
            cell.markup.push_str(open);
            cell.markup.push_str(&glib::markup_escape_text(text));
            cell.markup.push_str(close);
            cell.plain.push_str(text);
            return;
        }
        self.inline.push(tag);
        self.text(text);
        self.inline.pop();
    }

    fn push_inline(&mut self, tag: gtk::TextTag, open: &'static str, close: &'static str) {
        if let Some(cell) = self.cell() {
            cell.markup.push_str(open);
            self.markup_close.push(close);
        } else {
            self.inline.push(tag);
        }
    }

    fn pop_inline(&mut self) {
        if self.table.is_some() {
            if let Some(close) = self.markup_close.pop()
                && let Some(cell) = self.cell()
            {
                cell.markup.push_str(close);
            }
        } else {
            self.inline.pop();
        }
    }

    fn cell(&mut self) -> Option<&mut Cell> {
        self.table.as_mut()?.rows.last_mut()?.1.last_mut()
    }

    fn with_table(&mut self, f: impl FnOnce(&mut Table)) {
        if let Some(table) = &mut self.table {
            f(table);
        }
    }

    /// A formula, its scripts raised (or lowered) and smaller; a display formula on centred
    /// lines of its own.
    fn math(&mut self, latex: &str, display: bool) {
        let runs = math::runs(latex);
        if let Some(cell) = self.cell() {
            cell.markup.push_str(&format!("<span font_family=\"{}\">", fonts::MATH));
            for run in &runs {
                let script = match run.rise {
                    rise if rise > 0 => "sup",
                    rise if rise < 0 => "sub",
                    _ => "small",
                };
                let depth = usize::from(run.depth);
                let text = glib::markup_escape_text(&run.text.replace('\n', "; "));
                cell.markup.push_str(&format!("{}{text}{}", format!("<{script}>").repeat(depth), format!("</{script}>").repeat(depth)));
                cell.plain.push_str(&run.text);
            }
            cell.markup.push_str("</span>");
            return;
        }
        if display {
            self.end_line();
            self.block.push(self.styles.display_math.clone());
        } else {
            // Isolated like code, so a formula inside Persian keeps its order.
            self.text("\u{2068}");
        }
        self.inline.push(self.styles.math.clone());
        for run in &runs {
            let script = self.script(run.rise, run.depth);
            self.inline.extend(script.clone());
            self.text(&if display { run.text.clone() } else { run.text.replace('\n', "; ") });
            if script.is_some() {
                self.inline.pop();
            }
        }
        self.inline.pop();
        if display {
            self.end_line();
            self.block.pop();
        } else {
            self.text("\u{2069}");
        }
    }

    /// The tag for a formula's script `rise` fifths of an em up (down when negative) and
    /// `depth` scripts deep; none on the baseline.
    fn script(&mut self, rise: i8, depth: u8) -> Option<gtk::TextTag> {
        if rise == 0 && depth == 0 {
            return None;
        }
        let em = self.measure("M").max(8);
        let table = self.buffer.tag_table();
        let tag = self.scripts.entry((rise, depth)).or_insert_with(|| {
            let tag = gtk::TextTag::builder()
                .rise(i32::from(rise) * em * pango::SCALE / 5)
                .scale(if depth <= 1 { 0.72 } else { 0.58 })
                .build();
            table.add(&tag);
            tag
        });
        Some(tag.clone())
    }

    /// Writes the pending list bullet ("•", "◦", "▪" or "3.").
    fn bullet(&mut self) {
        self.pending_bullet = false;
        let depth = self.lists.len();
        let marker = match self.lists.last_mut().and_then(|l| l.next.as_mut()) {
            Some(n) => {
                *n += 1;
                format!("{}. ", *n - 1)
            }
            None => match depth {
                0 | 1 => "• ".to_owned(),
                2 => "◦ ".to_owned(),
                _ => "▪ ".to_owned(),
            },
        };
        self.hang(&marker);
        self.inline.push(self.styles.dim.clone());
        self.text(&marker);
        self.inline.pop();
    }

    /// Indents the open list item so its wrapped lines start under its text, not its marker.
    fn hang(&mut self, marker: &str) {
        let Some(&slot) = self.items.last() else { return };
        let indent = self.indent(self.lists.len(), self.measure(marker), false);
        self.block[slot] = indent;
    }

    fn task(&mut self, done: bool, range: Range<usize>) {
        self.pending_bullet = false;
        self.hang(if done { "☑ " } else { "☐ " });
        let colors = theme::colors();
        let tag = gtk::TextTag::builder().foreground((if done { colors.accent } else { colors.dim }).to_hex()).build();
        self.buffer.tag_table().add(&tag);
        self.targets.push((tag.clone(), Target::Task(range)));
        self.styled(if done { "☑ " } else { "☐ " }, tag, "", "");
        if done {
            self.inline.push(self.styles.done.clone());
        }
    }

    fn image(&mut self, url: &str, alt: &str) {
        if self.table.is_some() {
            return self.text(&format!("[image: {alt}]"));
        }
        let local = !url.contains("://");
        // "Pasted%20image.png" is the file "Pasted image.png", as in links; found beside the
        // note, or anywhere in the vault by its name, as Obsidian finds embeds.
        let path = links::decode(url);
        let file = || {
            (self.find)(&path)
                .or_else(|| self.folder.as_ref().map(|f| f.resolve_relative_path(&path)))
                .unwrap_or_else(|| gio::File::for_path(&path))
        };
        let texture = local.then(file).and_then(|file| gdk::Texture::from_file(&file).ok());
        // Obsidian's sizes: ![[pic.png|300]] (the text after | comes as the alt) and
        // ![caption|300x200](pic.png).
        let (alt, size) = match alt.rsplit_once('|') {
            Some((caption, size)) if picture_size(size).is_some() => (caption, picture_size(size)),
            _ => match picture_size(alt) {
                Some(size) => ("", Some(size)),
                None => (alt, None),
            },
        };
        let Some(texture) = texture else {
            // Remote or unreadable: show the alt text as a link instead.
            let label = if alt.is_empty() { url } else { alt };
            let tag = gtk::TextTag::builder().foreground(theme::colors().link.to_hex()).underline(pango::Underline::Single).build();
            self.buffer.tag_table().add(&tag);
            self.targets.push((tag.clone(), Target::Link(url.to_owned())));
            self.styled(&format!("[image: {label}]"), tag, "", "");
            return;
        };
        // Shown at most 720 px wide unless a size is given, keeping the aspect ratio unless
        // both sides are.
        let (w, h) = (texture.width().max(1), texture.height().max(1));
        let (width, height) = match size {
            Some((width, Some(height))) => (width, height),
            Some((width, None)) => (width, h * width / w),
            None => (w.min(720), h * w.min(720) / w),
        };
        let picture = gtk::Picture::for_paintable(&texture);
        picture.set_can_shrink(true);
        picture.set_size_request(width, height);
        if !alt.is_empty() {
            picture.set_alternative_text(Some(alt));
        }
        let mut end = self.buffer.end_iter();
        let anchor = self.buffer.create_child_anchor(&mut end);
        self.children.push((anchor, picture.upcast()));
    }

    /// A diagram on a centred line of its own.
    fn figure(&mut self, widget: gtk::Widget) {
        self.end_line();
        let mut end = self.buffer.end_iter();
        let anchor = self.buffer.create_child_anchor(&mut end);
        let mut start = end;
        start.backward_char();
        self.buffer.apply_tag(&self.styles.figure, &start, &end);
        self.children.push((anchor, widget));
        self.end_line();
        if self.quote_depth == 0 {
            self.gap();
        }
    }

    /// Ends the current line unless the buffer is empty or already at a line start.
    fn end_line(&mut self) {
        let mut end = self.buffer.end_iter();
        if end.offset() > 0 && end.backward_char() && end.char() != '\n' {
            let mut end = self.buffer.end_iter();
            self.buffer.insert(&mut end, "\n");
        }
    }

    /// A list item's indent: its depth's margin, with the first line pulled back by `hang`
    /// pixels so the marker sits in the margin and wrapped lines line up with the text.
    fn indent(&mut self, depth: usize, hang: i32, rtl: bool) -> gtk::TextTag {
        let table = self.buffer.tag_table();
        self.indents
            .entry((depth, hang, rtl))
            .or_insert_with(|| {
                let margin = self.margin + 6 + 22 * (depth.max(1) as i32 - 1) + hang;
                let tag = if rtl {
                    gtk::TextTag::builder().right_margin(margin).indent(-hang).pixels_below_lines(4).build()
                } else {
                    gtk::TextTag::builder().left_margin(margin).indent(-hang).pixels_below_lines(4).build()
                };
                table.add(&tag);
                tag
            })
            .clone()
    }

    /// An empty, short line: separates blocks whose background covers their own spacing.
    fn gap(&mut self) {
        self.end_line();
        let mut end = self.buffer.end_iter();
        self.buffer.insert_with_tags(&mut end, "\n", &[&self.styles.gap]);
    }

    /// Applies `tag` (spacing) to the line being written, from its start to the end.
    fn space_last_line(&self, tag: &gtk::TextTag) {
        let end = self.buffer.end_iter();
        let mut start = end;
        start.set_line_offset(0);
        self.buffer.apply_tag(tag, &start, &end);
    }

    /// Extra space above the first and below the last line of a code block.
    fn space_code_block(&self, start: i32) {
        let first = self.buffer.iter_at_offset(start);
        let mut first_end = first;
        first_end.forward_to_line_end();
        self.buffer.apply_tag(&self.styles.code_top, &first, &first_end);
        let mut last_end = self.buffer.end_iter();
        last_end.backward_char();
        let mut last = last_end;
        last.set_line_offset(0);
        self.buffer.apply_tag(&self.styles.code_bottom, &last, &last_end);
    }

    fn finish(mut self) -> Rendered {
        self.mirror_rtl_indents();
        // Drop the trailing newline so the view doesn't end with an empty line.
        let mut end = self.buffer.end_iter();
        let mut last = end;
        if last.backward_char() && last.char() == '\n' {
            self.buffer.delete(&mut last, &mut end);
        }
        self.buffer.set_modified(false);
        let start = self.buffer.start_iter();
        self.buffer.place_cursor(&start);
        Rendered {
            buffer: self.buffer,
            targets: self.targets,
            children: self.children,
            headings: self.headings,
            code: self.code,
        }
    }

    /// List indents are on the left; move them to the right on right-to-left lines.
    fn mirror_rtl_indents(&mut self) {
        let ltr: Vec<((usize, i32), gtk::TextTag)> = self
            .indents
            .iter()
            .filter(|((_, _, rtl), _)| !rtl)
            .map(|((depth, hang, _), tag)| ((*depth, *hang), tag.clone()))
            .collect();
        for line in 0..self.buffer.line_count() {
            let Some(start) = self.buffer.iter_at_line(line) else { continue };
            let Some(((depth, hang), tag)) = ltr.iter().find(|(_, tag)| start.has_tag(tag)) else { continue };
            let mut end = start;
            end.forward_to_line_end();
            if pango::find_base_dir(&self.buffer.text(&start, &end, false)) == pango::Direction::Rtl {
                let rtl = self.indent(*depth, *hang, true);
                self.buffer.remove_tag(tag, &start, &end);
                self.buffer.apply_tag(&rtl, &start, &end);
            }
        }
    }
}

/// Colours fenced code blocks with GtkSourceView's highlighting for their language. Meant to
/// run after the first frame: loading the language definitions takes ~100 ms.
pub fn highlight_code(buffer: &sourceview::Buffer, blocks: &[CodeBlock], scheme: Option<&sourceview::StyleScheme>) {
    let table = buffer.tag_table();
    let mut colours: HashMap<String, gtk::TextTag> = HashMap::new();
    for block in blocks {
        let start = buffer.iter_at_mark(&block.start);
        let code = buffer.text(&start, &buffer.iter_at_mark(&block.end), false);
        let base = start.offset() as usize;
        for (range, colour) in code_colours(&block.language, &code, scheme) {
            let tag = colours.entry(colour.to_string()).or_insert_with(|| {
                let tag = gtk::TextTag::builder().foreground_rgba(&colour).build();
                table.add(&tag);
                tag
            });
            let at = |offset: usize| buffer.iter_at_offset((base + offset) as i32);
            buffer.apply_tag(tag, &at(range.start), &at(range.end));
        }
    }
}

/// How GtkSourceView colours `code` as `language` (a fence's name: "rust", "py", "sh"…):
/// character ranges and their colours. Nothing for a language it doesn't know.
pub fn code_colours(language: &str, code: &str, scheme: Option<&sourceview::StyleScheme>) -> Vec<(Range<usize>, gdk::RGBA)> {
    let manager = sourceview::LanguageManager::default();
    let Some(language) = self::language(&manager, language) else { return Vec::new() };
    // Highlighted in a scratch buffer, its colours read back.
    let scratch = sourceview::Buffer::new(None);
    scratch.set_style_scheme(scheme);
    scratch.set_language(Some(&language));
    scratch.set_text(code);
    scratch.ensure_highlight(&scratch.start_iter(), &scratch.end_iter());
    let mut found = Vec::new();
    let mut iter = scratch.start_iter();
    loop {
        let from = iter.offset() as usize;
        let colour = foreground(&iter);
        let more = iter.forward_to_tag_toggle(None::<&gtk::TextTag>);
        if let Some(colour) = colour {
            found.push((from..iter.offset() as usize, colour));
        }
        if !more {
            break;
        }
    }
    found
}

/// The colour the highlighting gives the text at `iter`, if any.
fn foreground(iter: &gtk::TextIter) -> Option<gdk::RGBA> {
    iter.tags().into_iter().rev().find(|tag| tag.is_foreground_set()).and_then(|tag| tag.foreground_rgba())
}

/// GtkSourceView's language for a fence's name: its id, a common alias, or a file extension.
fn language(manager: &sourceview::LanguageManager, name: &str) -> Option<sourceview::Language> {
    let name = name.to_ascii_lowercase();
    let id = match name.as_str() {
        "py" | "python" => "python3",
        "js" | "javascript" => "js",
        "ts" => "typescript",
        "sh" | "bash" | "shell" | "zsh" | "console" => "sh",
        "rs" => "rust",
        "yml" => "yaml",
        "c++" => "cpp",
        "md" => "markdown",
        other => other,
    };
    manager.language(id).or_else(|| manager.guess_language(Some(format!("file.{name}")), None))
}

/// An Obsidian picture size: "300" (the width) or "300x200".
fn picture_size(text: &str) -> Option<(i32, Option<i32>)> {
    let number = |text: &str| text.trim().parse::<i32>().ok().filter(|&n| n > 0 && n <= 4000);
    match text.split_once('x') {
        Some((width, height)) => Some((number(width)?, Some(number(height)?))),
        None => Some((number(text)?, None)),
    }
}

fn heading_index(level: HeadingLevel) -> usize {
    level as usize - 1
}

/// Width in columns: CJK and other full-width characters count twice.
fn display_width(text: &str) -> usize {
    text.chars()
        .map(|c| match c as u32 {
            0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 => 2,
            _ => 1,
        })
        .sum()
}

fn table_widget(table: Table) -> gtk::Widget {
    let grid = gtk::Grid::builder()
        .column_spacing(18)
        .row_spacing(6)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(10)
        .margin_end(10)
        .build();
    for (row, (header, cells)) in table.rows.iter().enumerate() {
        for (col, cell) in cells.iter().enumerate() {
            let rtl = pango::find_base_dir(&cell.plain) == pango::Direction::Rtl;
            let xalign = match table.aligns.get(col) {
                Some(Alignment::Center) => 0.5,
                Some(Alignment::Right) => 1.0,
                Some(Alignment::Left) => 0.0,
                // Unaligned columns start where the cell's own text starts.
                _ if rtl => 1.0,
                _ => 0.0,
            };
            let markup = if *header { format!("<b>{}</b>", cell.markup) } else { cell.markup.clone() };
            // Embedded widgets get their minimum width, so only long cells may wrap.
            let long = display_width(&cell.plain) > 30;
            let label = gtk::Label::builder()
                .use_markup(true)
                .label(markup)
                .wrap(long)
                .width_chars(if long { 30 } else { -1 })
                .xalign(xalign)
                .build();
            grid.attach(&label, col as i32, row as i32, 1, 1);
        }
    }
    let frame = gtk::Frame::builder().child(&grid).margin_bottom(10).build();
    frame.add_css_class("nevis-table");
    frame.upcast()
}

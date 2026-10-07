//! Read mode: Markdown drawn as formatted text, with the markup itself hidden.

use std::collections::HashMap;
use std::ops::Range;

use gtk::{gdk, gio, glib, pango, prelude::*};
use pulldown_cmark::{Alignment, BlockQuoteKind, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};
use sourceview::prelude::*;

use crate::theme::palette;

/// What a click on the rendered text does.
#[derive(Clone)]
pub enum Target {
    /// A URL, a path relative to the file, or a `[[wiki]]` page name.
    Link(String),
    /// A task checkbox: the byte range of its `[ ]` / `[x]` in the Markdown source.
    Task(Range<usize>),
}

/// Side margins of the read view. Tag margins replace the view's, so indents add to this.
pub const MARGIN: i32 = 28;

pub struct Rendered {
    pub buffer: sourceview::Buffer,
    /// Clickable spans; each has its own tag.
    pub targets: Vec<(gtk::TextTag, Target)>,
    /// Images and tables, to attach once `buffer` is on a view.
    pub children: Vec<(gtk::TextChildAnchor, gtk::Widget)>,
}

/// Renders `source` into a new buffer. `folder` resolves relative image paths.
pub fn render(source: &str, folder: Option<&gio::File>, scheme: Option<&sourceview::StyleScheme>) -> Rendered {
    let buffer = sourceview::Buffer::new(None);
    buffer.set_style_scheme(scheme);
    buffer.set_highlight_syntax(false);
    let mut r = Renderer::new(folder, buffer);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_WIKILINKS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
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
}

impl Styles {
    fn new(table: &gtk::TextTagTable) -> Self {
        let add = |tag: gtk::TextTag| {
            table.add(&tag);
            tag
        };
        let heading = |scale: f64, above: i32| {
            add(gtk::TextTag::builder()
                .weight(700)
                .scale(scale)
                .foreground(palette::HEADING)
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
            code: add(gtk::TextTag::builder().family(palette::MONO).foreground(palette::CODE).background(palette::CODE_BG).build()),
            code_block: add(gtk::TextTag::builder()
                .family(palette::MONO)
                .foreground(palette::CODE)
                .paragraph_background(palette::CODE_BG)
                .direction(gtk::TextDirection::Ltr)
                .left_margin(MARGIN + 12)
                .right_margin(MARGIN + 12)
                .build()),
            code_top: add(gtk::TextTag::builder().pixels_above_lines(8).build()),
            code_bottom: add(gtk::TextTag::builder().pixels_below_lines(14).build()),
            quote: add(gtk::TextTag::builder()
                .paragraph_background(palette::QUOTE_BG)
                .foreground(palette::DIM_FG)
                .left_margin(MARGIN + 14)
                .right_margin(MARGIN + 14)
                .pixels_above_lines(2)
                .pixels_below_lines(2)
                .build()),
            callout: add(gtk::TextTag::builder().weight(700).foreground(palette::ACCENT).build()),
            done: add(gtk::TextTag::builder().strikethrough(true).foreground(palette::DIM).build()),
            dim: add(gtk::TextTag::builder().foreground(palette::DIM).build()),
            rule: add(gtk::TextTag::builder().size_points(1.0).paragraph_background(palette::RULE).build()),
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

struct Renderer {
    folder: Option<gio::File>,
    buffer: sourceview::Buffer,
    styles: Styles,
    /// Tags for everything inserted now: block-level ones, then inline ones.
    block: Vec<gtk::TextTag>,
    inline: Vec<gtk::TextTag>,
    lists: Vec<List>,
    quote_depth: usize,
    /// A list item started and its bullet is not written yet.
    pending_bullet: bool,
    /// Per-depth list indents, left-to-right and right-to-left variants.
    indents: HashMap<(usize, bool), gtk::TextTag>,
    code_start: Option<i32>,
    /// Image URL and alt text, while inside `![…](…)`.
    image: Option<(String, String)>,
    table: Option<Table>,
    /// Closing Pango tags for inline styles inside a table cell.
    markup_close: Vec<&'static str>,
    targets: Vec<(gtk::TextTag, Target)>,
    children: Vec<(gtk::TextChildAnchor, gtk::Widget)>,
}

impl Renderer {
    fn new(folder: Option<&gio::File>, buffer: sourceview::Buffer) -> Self {
        let styles = Styles::new(&buffer.tag_table());
        Self {
            folder: folder.cloned(),
            buffer,
            styles,
            block: Vec::new(),
            inline: Vec::new(),
            lists: Vec::new(),
            quote_depth: 0,
            pending_bullet: false,
            indents: HashMap::new(),
            code_start: None,
            image: None,
            table: None,
            markup_close: Vec::new(),
            targets: Vec::new(),
            children: Vec::new(),
        }
    }

    fn event(&mut self, event: Event, range: Range<usize>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            // U+2068…U+2069 isolate the span: `~/notes/a.md` inside Persian stays in order.
            Event::Code(text) => self.styled(&format!("\u{2068}{text}\u{2069}"), self.styles.code.clone(), "<tt>", "</tt>"),
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                self.styled(&text, self.styles.code.clone(), "<tt>", "</tt>")
            }
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
                let indent = self.indent(self.lists.len(), false);
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
                let tag = gtk::TextTag::builder()
                    .foreground(palette::LINK)
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
                self.end_line();
                self.block.pop();
                if let Some(start) = self.code_start.take() {
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
        self.inline.push(self.styles.dim.clone());
        self.text(&marker);
        self.inline.pop();
    }

    fn task(&mut self, done: bool, range: Range<usize>) {
        self.pending_bullet = false;
        let tag = gtk::TextTag::builder().foreground(if done { palette::ACCENT } else { palette::DIM }).build();
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
        let texture = local
            .then(|| self.folder.as_ref().map(|f| f.resolve_relative_path(url)).unwrap_or_else(|| gio::File::for_path(url)))
            .and_then(|file| gdk::Texture::from_file(&file).ok());
        let Some(texture) = texture else {
            // Remote or unreadable: show the alt text as a link instead.
            let label = if alt.is_empty() { url } else { alt };
            let tag = gtk::TextTag::builder().foreground(palette::LINK).underline(pango::Underline::Single).build();
            self.buffer.tag_table().add(&tag);
            self.targets.push((tag.clone(), Target::Link(url.to_owned())));
            self.styled(&format!("[image: {label}]"), tag, "", "");
            return;
        };
        // Shown at most 720 px wide, keeping the aspect ratio.
        let (w, h) = (texture.width().max(1), texture.height().max(1));
        let width = w.min(720);
        let picture = gtk::Picture::for_paintable(&texture);
        picture.set_can_shrink(true);
        picture.set_size_request(width, h * width / w);
        if !alt.is_empty() {
            picture.set_alternative_text(Some(alt));
        }
        let mut end = self.buffer.end_iter();
        let anchor = self.buffer.create_child_anchor(&mut end);
        self.children.push((anchor, picture.upcast()));
    }

    /// Ends the current line unless the buffer is empty or already at a line start.
    fn end_line(&mut self) {
        let mut end = self.buffer.end_iter();
        if end.offset() > 0 && end.backward_char() && end.char() != '\n' {
            let mut end = self.buffer.end_iter();
            self.buffer.insert(&mut end, "\n");
        }
    }

    fn indent(&mut self, depth: usize, rtl: bool) -> gtk::TextTag {
        let table = self.buffer.tag_table();
        self.indents
            .entry((depth, rtl))
            .or_insert_with(|| {
                let margin = MARGIN + 6 + 22 * (depth.max(1) as i32 - 1);
                let tag = if rtl {
                    gtk::TextTag::builder().right_margin(margin).pixels_below_lines(4).build()
                } else {
                    gtk::TextTag::builder().left_margin(margin).pixels_below_lines(4).build()
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
        Rendered { buffer: self.buffer, targets: self.targets, children: self.children }
    }

    /// List indents are on the left; move them to the right on right-to-left lines.
    fn mirror_rtl_indents(&mut self) {
        let ltr: Vec<(usize, gtk::TextTag)> =
            self.indents.iter().filter(|((_, rtl), _)| !rtl).map(|((d, _), t)| (*d, t.clone())).collect();
        for line in 0..self.buffer.line_count() {
            let Some(start) = self.buffer.iter_at_line(line) else { continue };
            let Some((depth, tag)) = ltr.iter().find(|(_, tag)| start.has_tag(tag)) else { continue };
            let mut end = start;
            end.forward_to_line_end();
            if pango::find_base_dir(&self.buffer.text(&start, &end, false)) == pango::Direction::Rtl {
                let rtl = self.indent(*depth, true);
                self.buffer.remove_tag(tag, &start, &end);
                self.buffer.apply_tag(&rtl, &start, &end);
            }
        }
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

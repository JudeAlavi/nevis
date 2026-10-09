//! Markdown styling for the source view: headings by level, dimmed markup characters,
//! bold, italic, links, code and quotes. The text stays exactly as typed; only its look changes.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::time::Instant;

use gtk::{gdk, glib, pango, prelude::*};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Tag, TagEnd};

use crate::devhooks;
use crate::render;
use crate::theme;

/// How a stretch of Markdown source is drawn. Where kinds overlap, the later one wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Quote,
    CodeBlock,
    Frontmatter,
    Html,
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
    Strong,
    Emphasis,
    Strike,
    Code,
    LinkText,
    ListMarker,
    Task,
    Url,
    Marker,
}

pub const KINDS: usize = Kind::Marker as usize + 1;

/// Every kind, in `Kind` order (which is also tag priority: later wins).
const ALL: [Kind; KINDS] = [
    Kind::Quote,
    Kind::CodeBlock,
    Kind::Frontmatter,
    Kind::Html,
    Kind::H1,
    Kind::H2,
    Kind::H3,
    Kind::H4,
    Kind::H5,
    Kind::H6,
    Kind::Strong,
    Kind::Emphasis,
    Kind::Strike,
    Kind::Code,
    Kind::LinkText,
    Kind::ListMarker,
    Kind::Task,
    Kind::Url,
    Kind::Marker,
];

/// Larger documents stay plain: they would be re-parsed on every keystroke.
const MAX_STYLED_BYTES: usize = 512 * 1024;

/// A code block's colours: character ranges and what colour each is.
type Colours = Rc<Vec<(Range<usize>, gdk::RGBA)>>;

/// Keeps a buffer's Markdown styled as it is edited.
pub struct Styler {
    buffer: sourceview::Buffer,
    /// One tag per kind, in `ALL` order.
    tags: Vec<gtk::TextTag>,
    enabled: Cell<bool>,
    queued: Cell<bool>,
    /// Fenced code coloured as its language: colours by language and code (so a block is
    /// worked out again only when it changes), a tag per colour, and what was coloured last.
    colours: RefCell<HashMap<(String, String), Colours>>,
    colour_tags: RefCell<HashMap<String, gtk::TextTag>>,
    coloured: RefCell<Vec<(String, String)>>,
    /// With spell checking on: the tag spell checking skips, kept on code and Persian.
    unspelled: RefCell<Option<gtk::TextTag>>,
}

impl Styler {
    pub fn new(buffer: &sourceview::Buffer) -> Rc<Self> {
        let table = buffer.tag_table();
        let tags = ALL
            .iter()
            .map(|&kind| {
                let tag = tag_for(kind);
                table.add(&tag);
                tag
            })
            .collect();
        let styler = Rc::new(Self {
            buffer: buffer.clone(),
            tags,
            enabled: Cell::new(false),
            queued: Cell::new(false),
            colours: RefCell::new(HashMap::new()),
            colour_tags: RefCell::new(HashMap::new()),
            coloured: RefCell::new(Vec::new()),
            unspelled: RefCell::new(None),
        });
        let weak = Rc::downgrade(&styler);
        buffer.connect_changed(move |_| {
            if let Some(styler) = weak.upgrade() {
                styler.queue();
            }
        });
        styler
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.get()
    }

    /// Whether `iter` is inside a code block or the front matter, where text is taken as is.
    pub fn in_code(&self, iter: &gtk::TextIter) -> bool {
        ALL.iter()
            .zip(&self.tags)
            .any(|(kind, tag)| matches!(kind, Kind::CodeBlock | Kind::Frontmatter) && iter.has_tag(tag))
    }

    /// Takes the colours of the palette now in use.
    pub fn recolor(&self) {
        for (&kind, tag) in ALL.iter().zip(&self.tags) {
            let fresh = tag_for(kind);
            tag.set_foreground_rgba(fresh.is_foreground_set().then(|| fresh.foreground_rgba()).flatten().as_ref());
            tag.set_background_rgba(fresh.is_background_set().then(|| fresh.background_rgba()).flatten().as_ref());
            tag.set_paragraph_background_rgba(
                fresh.is_paragraph_background_set().then(|| fresh.paragraph_background_rgba()).flatten().as_ref(),
            );
        }
        // Code takes the new scheme's colours.
        self.uncolour();
        self.colours.borrow_mut().clear();
        if self.enabled.get() {
            self.restyle();
        }
    }

    /// Styles the buffer from now on (for Markdown files), or removes all styling.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.set(on);
        if on {
            self.restyle();
        } else {
            self.clear();
        }
    }

    /// Restyles once the current batch of edits is done, before the next redraw,
    /// so typed text never shows unstyled for a frame.
    fn queue(self: &Rc<Self>) {
        if !self.enabled.get() || self.queued.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_full(glib::Priority::HIGH_IDLE, move || {
            if let Some(styler) = weak.upgrade() {
                styler.queued.set(false);
                styler.restyle();
            }
            glib::ControlFlow::Break
        });
    }

    /// Brings the tags in line with a fresh parse, touching only what changed:
    /// re-tagging the whole text would make GTK lay all of it out again.
    fn restyle(&self) {
        let started = Instant::now();
        let text = self.buffer.text(&self.buffer.start_iter(), &self.buffer.end_iter(), false);
        if text.len() > MAX_STYLED_BYTES {
            return self.clear();
        }
        let wanted = spans(&text);
        for (kind, tag) in ALL.iter().zip(&self.tags) {
            let have = tagged(&self.buffer, tag);
            let want = wanted.of(*kind);
            for range in subtract(&have, want) {
                self.buffer.remove_tag(tag, &self.iter_at(range.start), &self.iter_at(range.end));
            }
            for range in subtract(want, &have) {
                self.buffer.apply_tag(tag, &self.iter_at(range.start), &self.iter_at(range.end));
            }
        }
        self.colour_code(wanted.fences());
        if let Some(tag) = self.unspelled.borrow().as_ref() {
            let want = unspelled(&text, &wanted);
            let have = tagged(&self.buffer, tag);
            for range in subtract(&have, &want) {
                self.buffer.remove_tag(tag, &self.iter_at(range.start), &self.iter_at(range.end));
            }
            for range in subtract(&want, &have) {
                self.buffer.apply_tag(tag, &self.iter_at(range.start), &self.iter_at(range.end));
            }
        }
        if devhooks::active() {
            devhooks::lap(&format!("markdown styled in {:.1} ms", started.elapsed().as_secs_f64() * 1000.0));
            for (kind, tag) in ALL.iter().zip(&self.tags) {
                if tagged(&self.buffer, tag) != wanted.of(*kind) {
                    eprintln!("nevis: {kind:?} tags out of step with the text");
                }
            }
        }
    }

    /// Keeps `tag` on what spell checking should skip: code, addresses, and Persian (or any
    /// Arabic-script) words, which nevis has no dictionary for.
    pub fn skip_spelling(&self, tag: gtk::TextTag) {
        *self.unspelled.borrow_mut() = Some(tag);
        if self.is_enabled() {
            self.restyle();
        }
    }

    fn clear(&self) {
        let (start, end) = (self.buffer.start_iter(), self.buffer.end_iter());
        for tag in &self.tags {
            self.buffer.remove_tag(tag, &start, &end);
        }
        self.uncolour();
    }

    /// Colours each fenced code block as its language, when any code changed since last time
    /// (moving text around doesn't count: tags move with it).
    fn colour_code(&self, fences: &[Fence]) {
        let codes: Vec<(String, String)> = fences
            .iter()
            .map(|fence| {
                let code = fence
                    .pieces
                    .iter()
                    .map(|p| self.buffer.text(&self.iter_at(p.start), &self.iter_at(p.end), false).to_string())
                    .collect();
                (fence.language.clone(), code)
            })
            .collect();
        if *self.coloured.borrow() == codes {
            return;
        }
        self.uncolour();
        let scheme = theme::scheme();
        let mut cache = self.colours.borrow_mut();
        cache.retain(|key, _| codes.contains(key));
        for (fence, key) in fences.iter().zip(&codes) {
            let colours = cache
                .entry(key.clone())
                .or_insert_with(|| Rc::new(render::code_colours(&key.0, &key.1, scheme.as_ref())))
                .clone();
            for (range, colour) in colours.iter() {
                let tag = self.colour_tag(colour);
                // The code is the pieces one after another; back to where each piece is.
                let mut before = 0;
                for piece in &fence.pieces {
                    let len = piece.end - piece.start;
                    let (from, to) = (range.start.max(before), range.end.min(before + len));
                    if from < to {
                        let at = |offset: usize| self.iter_at(piece.start + offset - before);
                        self.buffer.apply_tag(&tag, &at(from), &at(to));
                    }
                    before += len;
                }
            }
        }
        *self.coloured.borrow_mut() = codes;
    }

    fn colour_tag(&self, colour: &gdk::RGBA) -> gtk::TextTag {
        self.colour_tags
            .borrow_mut()
            .entry(colour.to_string())
            .or_insert_with(|| {
                let tag = gtk::TextTag::builder().foreground_rgba(colour).build();
                self.buffer.tag_table().add(&tag);
                tag
            })
            .clone()
    }

    /// Takes the code colours off, everywhere.
    fn uncolour(&self) {
        let (start, end) = (self.buffer.start_iter(), self.buffer.end_iter());
        for tag in self.colour_tags.borrow().values() {
            self.buffer.remove_tag(tag, &start, &end);
        }
        self.coloured.borrow_mut().clear();
    }

    fn iter_at(&self, offset: usize) -> gtk::TextIter {
        self.buffer.iter_at_offset(offset as i32)
    }
}

/// How each kind looks: Ayu Mirage colours, sizes for headings.
fn tag_for(kind: Kind) -> gtk::TextTag {
    let heading = |scale: f64| gtk::TextTag::builder().weight(700).scale(scale).foreground(theme::colors().heading.to_hex()).build();
    match kind {
        Kind::Quote => gtk::TextTag::builder().foreground(theme::colors().quote.to_hex()).style(pango::Style::Italic).build(),
        Kind::CodeBlock => gtk::TextTag::builder()
            .foreground(theme::colors().code.to_hex())
            .paragraph_background(theme::colors().code_background.to_hex())
            .direction(gtk::TextDirection::Ltr)
            .build(),
        Kind::Frontmatter | Kind::Html | Kind::Url => gtk::TextTag::builder().foreground(theme::colors().dim.to_hex()).build(),
        Kind::H1 => heading(1.6),
        Kind::H2 => heading(1.4),
        Kind::H3 => heading(1.25),
        Kind::H4 => heading(1.12),
        Kind::H5 => heading(1.05),
        Kind::H6 => heading(1.0),
        Kind::Strong => gtk::TextTag::builder().weight(700).build(),
        Kind::Emphasis => gtk::TextTag::builder().style(pango::Style::Italic).build(),
        Kind::Strike => gtk::TextTag::builder().strikethrough(true).foreground(theme::colors().dim.to_hex()).build(),
        Kind::Code => gtk::TextTag::builder().foreground(theme::colors().code.to_hex()).background(theme::colors().code_background.to_hex()).build(),
        Kind::LinkText => gtk::TextTag::builder().foreground(theme::colors().link.to_hex()).underline(pango::Underline::Single).build(),
        Kind::ListMarker => gtk::TextTag::builder().foreground(theme::colors().bullet.to_hex()).weight(700).build(),
        Kind::Task => gtk::TextTag::builder().foreground(theme::colors().accent.to_hex()).weight(700).build(),
        Kind::Marker => gtk::TextTag::builder().foreground(theme::colors().marker.to_hex()).build(),
    }
}

/// What spell checking skips in `text`, styled as `spans`: code, links' addresses and
/// written-out ones, front matter, HTML and Arabic-script words; character ranges, sorted and
/// merged.
fn unspelled(text: &str, spans: &Spans) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = [Kind::Code, Kind::CodeBlock, Kind::Url, Kind::Frontmatter, Kind::Html]
        .iter()
        .flat_map(|&kind| spans.of(kind).iter().cloned())
        .collect();
    let arabic = |c: char| matches!(c, '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}' | '\u{08A0}'..='\u{08FF}'
        | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}' | '\u{200C}');
    let mut start = None;
    for (offset, c) in text.chars().enumerate() {
        match (arabic(c), start) {
            (true, None) => start = Some(offset),
            (false, Some(from)) => {
                ranges.push(from..offset);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        ranges.push(from..text.chars().count());
    }
    // Addresses written out (https://…, www.…): Markdown only marks <https://…> as links.
    let (mut word, mut chars) = (None, 0);
    for (byte, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        if c.is_whitespace() {
            if let Some((from, from_byte)) = word.take() {
                let written = &text[from_byte..byte];
                if written.contains("://") || written.starts_with("www.") {
                    ranges.push(from..chars);
                }
            }
        } else if word.is_none() {
            word = Some((chars, byte));
        }
        chars += 1;
    }
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// The character ranges `tag` currently covers, in order.
fn tagged(buffer: &sourceview::Buffer, tag: &gtk::TextTag) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut iter = buffer.start_iter();
    let mut open = iter.has_tag(tag).then_some(0);
    while iter.forward_to_tag_toggle(Some(tag)) {
        let offset = iter.offset() as usize;
        match open.take() {
            Some(start) => ranges.push(start..offset),
            None => open = Some(offset),
        }
    }
    if let Some(start) = open {
        ranges.push(start..buffer.char_count() as usize);
    }
    ranges
}

/// The parts of `a` not covered by `b`; both sorted and non-overlapping.
fn subtract(a: &[Range<usize>], b: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut first = 0;
    for range in a {
        while first < b.len() && b[first].end <= range.start {
            first += 1;
        }
        let mut start = range.start;
        for cut in b[first..].iter().take_while(|cut| cut.start < range.end) {
            if cut.start > start {
                out.push(start..cut.start);
            }
            start = start.max(cut.end);
        }
        if start < range.end {
            out.push(start..range.end);
        }
    }
    out
}

/// Character ranges (not bytes) for each kind, sorted and merged; and the fenced code blocks
/// that name a language, to colour as that language.
#[derive(Debug, Default)]
pub struct Spans([Vec<Range<usize>>; KINDS], Vec<Fence>);

/// A fenced code block's language, and the character ranges its code is in: one, or a piece
/// per line inside a quote or a list (their markers aren't code).
#[derive(Clone, Debug, PartialEq)]
pub struct Fence {
    pub language: String,
    pub pieces: Vec<Range<usize>>,
}

impl Spans {
    pub fn of(&self, kind: Kind) -> &[Range<usize>] {
        &self.0[kind as usize]
    }

    pub fn fences(&self) -> &[Fence] {
        &self.1
    }
}

/// Works out how each part of `source` should look.
pub fn spans(source: &str) -> Spans {
    let mut out = Collector::default();
    // Open links and images, each with the extent of its label so far.
    let mut links: Vec<(Range<usize>, Option<Range<usize>>)> = Vec::new();
    // The fenced code block being read, if it names a language, and its code so far.
    let mut fence: Option<(String, Vec<Range<usize>>)> = None;
    for (event, range) in render::events(source) {
        match &event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                // "rust", "py title", "rust,ignore", "{.python}".
                let language = info.split([' ', '\t', ',']).next().unwrap_or_default().trim_matches(['{', '}', '.']);
                fence = (!language.is_empty()).then(|| (language.to_owned(), Vec::new()));
            }
            Event::Text(_) => {
                if let Some((_, pieces)) = &mut fence {
                    pieces.push(range.clone());
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((language, pieces)) = fence.take() {
                    out.fences.push((language, pieces));
                }
            }
            _ => {}
        }
        if let Event::End(TagEnd::Link | TagEnd::Image) = event {
            if let Some((link, label)) = links.pop() {
                link_parts(link, label, &mut out);
            }
            continue;
        }
        // Whatever sits inside a link is its label.
        if let Some((_, label)) = links.last_mut() {
            *label = Some(match label.take() {
                Some(l) => l.start.min(range.start)..l.end.max(range.end),
                None => range.clone(),
            });
        }
        match event {
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => links.push((range, None)),
            Event::Start(Tag::Heading { level, .. }) => heading(source, range, level, &mut out),
            Event::Start(Tag::Strong) => wrapped(Kind::Strong, range, 2, &mut out),
            Event::Start(Tag::Emphasis) => wrapped(Kind::Emphasis, range, 1, &mut out),
            Event::Start(Tag::Strikethrough) => {
                let tildes = source[range.clone()].bytes().take_while(|&b| b == b'~').count();
                wrapped(Kind::Strike, range, tildes, &mut out);
            }
            Event::Code(_) => {
                let ticks = source[range.clone()].bytes().take_while(|&b| b == b'`').count();
                wrapped(Kind::Code, range, ticks, &mut out);
            }
            Event::InlineMath(_) => wrapped(Kind::Code, range, 1, &mut out),
            Event::DisplayMath(_) => wrapped(Kind::Code, range, 2, &mut out),
            Event::Start(Tag::CodeBlock(kind)) => code_block(source, range, kind.is_fenced(), &mut out),
            Event::Start(Tag::Item) => list_marker(source, range, &mut out),
            Event::Start(Tag::BlockQuote(_)) => quote(source, range, &mut out),
            Event::Start(Tag::Table(_)) => table(source, range, &mut out),
            Event::Start(Tag::TableHead) => out.push(Kind::Strong, without_newline(source, range)),
            Event::Start(Tag::MetadataBlock(_)) => out.push(Kind::Frontmatter, without_newline(source, range)),
            Event::Rule => out.push(Kind::Marker, without_newline(source, range)),
            Event::Start(Tag::HtmlBlock) | Event::InlineHtml(_) => {
                out.push(Kind::Html, without_newline(source, range));
            }
            Event::TaskListMarker(_) => out.push(Kind::Task, range),
            _ => {}
        }
    }
    out.into_spans(source)
}

/// The quote in quote style; the "> " (or "> > ") at the start of each line dimmed.
fn quote(source: &str, range: Range<usize>, out: &mut Collector) {
    let block = without_newline(source, range);
    out.push(Kind::Quote, block.clone());
    let mut line_start = block.start;
    for line in source[block.clone()].split_inclusive('\n') {
        let bytes = line.as_bytes();
        let mut i = 0;
        loop {
            while bytes.get(i) == Some(&b' ') {
                i += 1;
            }
            if bytes.get(i) != Some(&b'>') {
                break;
            }
            let marker = i;
            i += 1;
            if bytes.get(i) == Some(&b' ') {
                i += 1;
            }
            out.push(Kind::Marker, line_start + marker..line_start + i);
        }
        line_start += line.len();
    }
}

/// The pipes between cells and the "|---|" line under the header, dimmed.
fn table(source: &str, range: Range<usize>, out: &mut Collector) {
    let block = without_newline(source, range);
    let mut line_start = block.start;
    for (index, line) in source[block.clone()].split_inclusive('\n').enumerate() {
        if index == 1 {
            let divider = line.trim_end_matches(['\n', '\r']);
            out.push(Kind::Marker, line_start..line_start + divider.len());
        } else {
            for (i, _) in line.match_indices('|') {
                out.push(Kind::Marker, line_start + i..line_start + i + 1);
            }
        }
        line_start += line.len();
    }
}

/// The "-", "*", "+", "1." or "1)" that starts a list item.
fn list_marker(source: &str, item: Range<usize>, out: &mut Collector) {
    let text = &source[item.clone()];
    let indent = text.len() - text.trim_start_matches(' ').len();
    let rest = &text[indent..];
    let len = if rest.starts_with(['-', '*', '+']) {
        1
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && rest[digits..].starts_with(['.', ')']) { digits + 1 } else { 0 }
    };
    let start = item.start + indent;
    out.push(Kind::ListMarker, start..start + len);
}

/// "[label](url)": the label as a link, what comes before it dimmed, the rest as the URL.
fn link_parts(link: Range<usize>, label: Option<Range<usize>>, out: &mut Collector) {
    match label {
        Some(label) => {
            out.push(Kind::Marker, link.start..label.start);
            out.push(Kind::LinkText, label.clone());
            out.push(Kind::Url, label.end..link.end);
        }
        None => out.push(Kind::Url, link),
    }
}

/// The whole block in code style; for a fenced block, its opening and closing fences dimmed.
fn code_block(source: &str, range: Range<usize>, fenced: bool, out: &mut Collector) {
    let block = without_newline(source, range);
    out.push(Kind::CodeBlock, block.clone());
    if !fenced {
        return;
    }
    let text = &source[block.clone()];
    let first_line = text.find('\n').unwrap_or(text.len());
    out.push(Kind::Marker, block.start..block.start + first_line);
    if let Some(last_newline) = text.rfind('\n') {
        let last_line = text[last_newline + 1..].trim_start();
        if last_line.starts_with("```") || last_line.starts_with("~~~") {
            out.push(Kind::Marker, block.start + last_newline + 1..block.end);
        }
    }
}

/// "**bold**": the whole span in `kind`, the `len` delimiter characters at each end dimmed.
fn wrapped(kind: Kind, range: Range<usize>, len: usize, out: &mut Collector) {
    out.push(kind, range.clone());
    out.push(Kind::Marker, range.start..(range.start + len).min(range.end));
    out.push(Kind::Marker, range.end.saturating_sub(len).max(range.start)..range.end);
}

/// "## Title": the line in its level's style, the hashes and the space after them dimmed.
/// Also dims closing hashes ("# Title #") and the "===" / "---" under a two-line heading.
fn heading(source: &str, range: Range<usize>, level: HeadingLevel, out: &mut Collector) {
    let line = without_newline(source, range);
    out.push(heading_kind(level), line.clone());
    let text = &source[line.clone()];
    let indent = text.len() - text.trim_start_matches(' ').len();
    let hashes = text[indent..].bytes().take_while(|&b| b == b'#').count();
    if hashes == 0 {
        if let Some(newline) = text.rfind('\n') {
            out.push(Kind::Marker, line.start + newline + 1..line.end);
        }
        return;
    }
    let spaces = text[indent + hashes..].bytes().take_while(|&b| b == b' ' || b == b'\t').count();
    out.push(Kind::Marker, line.start + indent..line.start + indent + hashes + spaces);
    // Trailing hashes close the heading only when a space separates them from the title.
    let body = text.trim_end();
    let closing = body.bytes().rev().take_while(|&b| b == b'#').count();
    let before = body.len() - closing;
    if closing > 0 && before > indent + hashes && body.as_bytes()[before - 1] == b' ' {
        out.push(Kind::Marker, line.start + before..line.start + body.len());
    }
}

fn heading_kind(level: HeadingLevel) -> Kind {
    match level {
        HeadingLevel::H1 => Kind::H1,
        HeadingLevel::H2 => Kind::H2,
        HeadingLevel::H3 => Kind::H3,
        HeadingLevel::H4 => Kind::H4,
        HeadingLevel::H5 => Kind::H5,
        HeadingLevel::H6 => Kind::H6,
    }
}

fn without_newline(source: &str, range: Range<usize>) -> Range<usize> {
    let text = &source[range.clone()];
    range.start..range.start + text.trim_end_matches(['\n', '\r']).len()
}

/// Byte ranges per kind, collected while walking the parse.
#[derive(Default)]
struct Collector {
    ranges: [Vec<Range<usize>>; KINDS],
    /// Fenced code: language and byte ranges.
    fences: Vec<(String, Vec<Range<usize>>)>,
}

impl Collector {
    fn push(&mut self, kind: Kind, range: Range<usize>) {
        if range.start < range.end {
            self.ranges[kind as usize].push(range);
        }
    }

    /// Character ranges, sorted, with overlapping or touching ones merged.
    fn into_spans(self, source: &str) -> Spans {
        let starts: Vec<usize> = source.char_indices().map(|(i, _)| i).collect();
        let char_at = |byte: usize| starts.partition_point(|&start| start < byte);
        let fences = self
            .fences
            .into_iter()
            .map(|(language, pieces)| Fence { language, pieces: pieces.into_iter().map(|p| char_at(p.start)..char_at(p.end)).collect() })
            .collect();
        let ranges = self.ranges.map(|mut ranges| {
            ranges.sort_by_key(|r| r.start);
            let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
            for range in ranges {
                let range = char_at(range.start)..char_at(range.end);
                match merged.last_mut() {
                    Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                    _ => merged.push(range),
                }
            }
            merged
        });
        Spans(ranges, fences)
    }
}

#[cfg(test)]
// `&[0..8]` here is meant: a list holding one range.
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    #[test]
    fn heading_gets_its_level_and_dims_the_hashes() {
        let s = spans("## Title\n");
        assert_eq!(s.of(Kind::H2), &[0..8]);
        assert_eq!(s.of(Kind::Marker), &[0..3]);
    }

    #[test]
    fn bold_text_is_bold_and_its_stars_are_dimmed() {
        let s = spans("a **bold** b");
        assert_eq!(s.of(Kind::Strong), &[2..10]);
        assert_eq!(s.of(Kind::Marker), &[2..4, 8..10]);
    }

    #[test]
    fn italic_and_struck_text_dim_their_delimiters() {
        let s = spans("*it* ~~no~~");
        assert_eq!(s.of(Kind::Emphasis), &[0..4]);
        assert_eq!(s.of(Kind::Strike), &[5..11]);
        assert_eq!(s.of(Kind::Marker), &[0..1, 3..4, 5..7, 9..11]);
    }

    #[test]
    fn inline_code_dims_its_backticks_however_many() {
        let s = spans("use `ls -l` now");
        assert_eq!(s.of(Kind::Code), &[4..11]);
        assert_eq!(s.of(Kind::Marker), &[4..5, 10..11]);

        let s = spans("``a`b``");
        assert_eq!(s.of(Kind::Code), &[0..7]);
        assert_eq!(s.of(Kind::Marker), &[0..2, 5..7]);
    }

    #[test]
    fn math_is_styled_like_code_with_its_dollars_dimmed() {
        let s = spans("so $x^2$ and $$y$$");
        assert_eq!(s.of(Kind::Code), &[3..8, 13..18]);
        assert_eq!(s.of(Kind::Marker), &[3..4, 7..8, 13..15, 16..18]);
    }

    fn fence(language: &str, pieces: &[Range<usize>]) -> Fence {
        Fence { language: language.to_owned(), pieces: pieces.to_vec() }
    }

    #[test]
    fn fenced_code_names_its_language_and_where_its_code_is() {
        // "```rust\n" starts at 5, so the code "fn main() {}\n" is 13..26; "py title" is py.
        let s = spans("Text\n```rust\nfn main() {}\n```\n\n~~~py title\nx = 1\n~~~\n```\nno language\n```\n");
        assert_eq!(s.fences(), &[fence("rust", &[13..26]), fence("py", &[43..49])]);
        // In a quote, each line's code without its "> ".
        let s = spans("> ```sh\n> ls -l\n> echo hi\n> ```\n");
        assert_eq!(s.fences(), &[fence("sh", &[10..16, 18..26])]);
    }

    #[test]
    fn fenced_code_block_is_one_block_with_dimmed_fences() {
        let s = spans("```rust\nfn main() {}\n```\n");
        assert_eq!(s.of(Kind::CodeBlock), &[0..24]);
        assert_eq!(s.of(Kind::Marker), &[0..7, 21..24]);
    }

    #[test]
    fn link_label_stands_out_and_the_url_is_dimmed() {
        let s = spans("see [docs](https://x.io) ok");
        assert_eq!(s.of(Kind::LinkText), &[5..9]);
        assert_eq!(s.of(Kind::Marker), &[4..5]);
        assert_eq!(s.of(Kind::Url), &[9..24]);
    }

    #[test]
    fn wikilinks_autolinks_images_and_formatted_labels_split_the_same_way() {
        let s = spans("[[Other Note]]");
        assert_eq!(s.of(Kind::Marker), &[0..2]);
        assert_eq!(s.of(Kind::LinkText), &[2..12]);
        assert_eq!(s.of(Kind::Url), &[12..14]);

        let s = spans("<https://a.b>");
        assert_eq!(s.of(Kind::Marker), &[0..1]);
        assert_eq!(s.of(Kind::LinkText), &[1..12]);
        assert_eq!(s.of(Kind::Url), &[12..13]);

        let s = spans("![alt](p.png)");
        assert_eq!(s.of(Kind::Marker), &[0..2]);
        assert_eq!(s.of(Kind::LinkText), &[2..5]);
        assert_eq!(s.of(Kind::Url), &[5..13]);

        let s = spans("[**b**](u)");
        assert_eq!(s.of(Kind::LinkText), &[1..6]);
        assert_eq!(s.of(Kind::Strong), &[1..6]);
        assert_eq!(s.of(Kind::Marker), &[0..3, 4..6]);
        assert_eq!(s.of(Kind::Url), &[6..10]);
    }

    #[test]
    fn list_bullets_numbers_and_task_boxes_are_marked() {
        let s = spans("- [ ] buy milk\n1. first\n");
        assert_eq!(s.of(Kind::ListMarker), &[0..1, 15..17]);
        assert_eq!(s.of(Kind::Task), &[2..5]);
    }

    #[test]
    fn quote_lines_are_quoted_and_their_markers_dimmed() {
        let s = spans("> hello\n> world\n");
        assert_eq!(s.of(Kind::Quote), &[0..15]);
        assert_eq!(s.of(Kind::Marker), &[0..2, 8..10]);
    }

    #[test]
    fn table_header_is_bold_and_pipes_and_divider_are_dimmed() {
        let s = spans("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert_eq!(s.of(Kind::Strong), &[0..9]);
        assert_eq!(s.of(Kind::Marker), &[0..1, 4..5, 8..9, 10..19, 20..21, 24..25, 28..29]);
    }

    #[test]
    fn frontmatter_is_one_dimmed_block() {
        let s = spans("---\ntitle: x\n---\n\nbody\n");
        assert_eq!(s.of(Kind::Frontmatter), &[0..16]);
    }

    #[test]
    fn a_rule_further_down_is_a_rule_even_with_text_right_under_it() {
        // Front matter is only ever at the top; this `---` and the next are rules.
        let s = spans("a\n\n---\n# Plans\n---\n");
        assert!(s.of(Kind::Frontmatter).is_empty());
        assert_eq!(s.of(Kind::H1), &[7..14]);
        assert_eq!(s.of(Kind::Marker), &[3..6, 7..9, 15..18]);
    }

    #[test]
    fn rules_and_html_are_dimmed() {
        let s = spans("a\n\n---\n\nb <br> c\n");
        assert_eq!(s.of(Kind::Marker), &[3..6]);
        assert_eq!(s.of(Kind::Html), &[10..14]);
    }

    #[test]
    fn ranges_count_characters_so_persian_text_lines_up() {
        let s = spans("## سلام\n**مهم** و `کد`");
        assert_eq!(s.of(Kind::H2), &[0..7]);
        assert_eq!(s.of(Kind::Strong), &[8..15]);
        assert_eq!(s.of(Kind::Code), &[18..22]);
        assert_eq!(s.of(Kind::Marker), &[0..3, 8..10, 13..15, 18..19, 21..22]);
    }

    #[test]
    fn closing_hashes_and_underlines_are_dimmed_but_a_hash_in_the_title_is_not() {
        let s = spans("# Title #\n");
        assert_eq!(s.of(Kind::Marker), &[0..2, 8..9]);

        let s = spans("# C#\n");
        assert_eq!(s.of(Kind::Marker), &[0..2]);

        let s = spans("Title\n=====\n");
        assert_eq!(s.of(Kind::H1), &[0..11]);
        assert_eq!(s.of(Kind::Marker), &[6..11]);
    }
}

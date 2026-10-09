//! The text views' paper: what nevis draws behind and over the text. Behind it, each block's
//! panel: a callout's box, a bar down a quote's or an embedded note's side. A panel spans its
//! whole block whatever the margins of its lines; a paragraph background starts at each line's
//! own margin, so a list in a callout would leave notches in it. Over the text, in Live
//! Preview: dots, checkboxes, rules and callout titles where the markup under them is; and over
//! whole blocks (tables, pictures, flowcharts, formulas), the block drawn, its lines covered
//! (they stay, so each keeps its line number and the cursor goes into them as into any).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gtk::{gdk, gio, glib, graphene, gsk, pango, prelude::*, subclass::prelude::*};
use pulldown_cmark::Alignment;
use sourceview::subclass::prelude::*;

use crate::render::{self, Look, Panel};
use crate::style::{Block, Mark};
use crate::theme::{self, fonts};
use crate::{devhooks, flowchart};

/// Finds a file a link names (as the rendered view does).
type Finder = Box<dyn Fn(&str) -> Option<gio::File>>;

/// Drawings by a block's kind, source and the text size (none where it can't be drawn).
type Drawings = HashMap<(Block, String, i32), Option<Rc<Drawing>>>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Paper {
        pub panels: RefCell<Vec<Panel>>,
        pub marks: RefCell<Vec<(Range<usize>, Mark)>>,
        /// Live Preview's blocks: those asked for (lines, kind, source), those drawn, and the
        /// room kept above the first line of those taller drawn than written.
        pub wanted: RefCell<Vec<(Range<usize>, Block, String)>>,
        pub blocks: RefCell<Vec<(Range<usize>, Rc<Drawing>)>>,
        pub applied: RefCell<Vec<(Range<usize>, i32)>>,
        pub drawings: RefCell<Drawings>,
        /// Tags keeping room above a line, by height.
        pub rooms: RefCell<HashMap<i32, gtk::TextTag>>,
        pub finder: RefCell<Option<Finder>>,
        /// The width and text size blocks were last placed for.
        pub width: Cell<i32>,
        pub size: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Paper {
        const NAME: &'static str = "NevisPaper";
        type Type = super::Paper;
        type ParentType = sourceview::View;
    }

    impl ObjectImpl for Paper {}
    impl ViewImpl for Paper {}

    impl WidgetImpl for Paper {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            // Drawn blocks fit the new width (pictures shrink): their room again, once allocated.
            if self.width.replace(width) != width && !self.wanted.borrow().is_empty() {
                let paper = self.obj().downgrade();
                glib::idle_add_local_once(move || {
                    if let Some(paper) = paper.upgrade() {
                        paper.place_blocks();
                    }
                });
            }
        }
    }

    impl TextViewImpl for Paper {
        fn snapshot_layer(&self, layer: gtk::TextViewLayer, snapshot: gtk::Snapshot) {
            if layer == gtk::TextViewLayer::BelowText {
                super::draw_panels(&self.obj(), &self.panels.borrow(), &snapshot);
            }
            self.parent_snapshot_layer(layer, snapshot.clone());
            if layer == gtk::TextViewLayer::AboveText {
                super::draw_blocks(&self.obj(), &self.blocks.borrow(), &snapshot);
                super::draw_marks(&self.obj(), &self.marks.borrow(), &snapshot);
                // The text size changed: blocks drawn again at the new size, once this is drawn.
                let size = self.obj().pango_context().font_description().map_or(0, |font| font.size());
                if size != self.size.get() && !self.wanted.borrow().is_empty() {
                    self.size.set(size);
                    let paper = self.obj().downgrade();
                    glib::idle_add_local_once(move || {
                        if let Some(paper) = paper.upgrade() {
                            paper.place_blocks();
                        }
                    });
                }
            }
        }
    }
}

glib::wrapper! {
    pub struct Paper(ObjectSubclass<imp::Paper>)
        @extends sourceview::View, gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl Paper {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Draws `panels`, marked in the buffer shown now, behind the text (the marks of those
    /// drawn before go).
    pub fn set_panels(&self, panels: Vec<Panel>) {
        let old = self.imp().panels.replace(panels);
        for panel in old {
            for mark in [panel.start, panel.end].into_iter().chain(panel.fold.map(|(_, body)| body)) {
                if let Some(buffer) = mark.buffer() {
                    buffer.delete_mark(&mark);
                }
            }
        }
        self.queue_draw();
    }

    /// Draws `marks` (sorted character ranges of the buffer shown now) over the text.
    pub fn set_marks(&self, marks: Vec<(Range<usize>, Mark)>) {
        let mut have = self.imp().marks.borrow_mut();
        if *have != marks {
            *have = marks;
            self.queue_draw();
        }
    }

    /// Finds the files pictures name with `finder`.
    pub fn find_with(&self, finder: impl Fn(&str) -> Option<gio::File> + 'static) {
        *self.imp().finder.borrow_mut() = Some(Box::new(finder));
    }

    /// Draws `blocks` (whole lines, sorted characters of the buffer shown now; see
    /// `style::blocks`) over their lines: those it can draw.
    pub fn set_blocks(&self, blocks: Vec<(Range<usize>, Block, String)>) {
        *self.imp().wanted.borrow_mut() = blocks;
        self.place_blocks();
    }

    /// Draws everything afresh (new colours).
    pub fn forget_drawings(&self) {
        self.imp().drawings.borrow_mut().clear();
        self.imp().applied.borrow_mut().clear();
        self.place_blocks();
    }

    /// Keeps room above the first line of each block that is drawn taller than it is written.
    fn place_blocks(&self) {
        let imp = self.imp();
        imp.size.set(self.pango_context().font_description().map_or(0, |font| font.size()));
        let buffer = self.buffer();
        let room = (self.width() - self.left_margin() - self.right_margin()).max(64) as f32;
        let mut drawn = Vec::new();
        let mut applied = Vec::new();
        for (range, block, source) in imp.wanted.borrow().iter() {
            let Some(drawing) = self.drawing(*block, source) else { continue };
            let (first, last) = (buffer.iter_at_offset(range.start as i32), buffer.iter_at_offset(range.end as i32));
            let (top, bottom) = (place(self, &first), place(self, &last));
            let written = bottom.y() + bottom.height() - top.y();
            let keep = (drawing.size(room).1.ceil() as i32 + ABOVE + BELOW - written).max(0);
            applied.push((range.clone(), keep));
            drawn.push((range.clone(), drawing));
        }
        if *imp.applied.borrow() != applied {
            let (start, end) = buffer.bounds();
            for tag in imp.rooms.borrow().values() {
                buffer.remove_tag(tag, &start, &end);
            }
            for (range, keep) in applied.iter().filter(|(_, keep)| *keep > 0) {
                let first = buffer.iter_at_offset(range.start as i32);
                let mut next = first;
                next.forward_line();
                let keep = *keep;
                let tag = imp
                    .rooms
                    .borrow_mut()
                    .entry(keep)
                    .or_insert_with(|| {
                        let tag = gtk::TextTag::builder().pixels_above_lines(keep).build();
                        buffer.tag_table().add(&tag);
                        tag
                    })
                    .clone();
                buffer.apply_tag(&tag, &first, &next);
            }
            *imp.applied.borrow_mut() = applied;
        }
        *imp.blocks.borrow_mut() = drawn;
        self.queue_draw();
    }

    /// What `source` (a `block`) is drawn as, made once.
    fn drawing(&self, block: Block, source: &str) -> Option<Rc<Drawing>> {
        let size = self.pango_context().font_description().map_or(0, |font| font.size());
        let key = (block, source.to_owned(), size);
        if let Some(found) = self.imp().drawings.borrow().get(&key) {
            return found.clone();
        }
        let started = std::time::Instant::now();
        let made = self.make(block, source).map(Rc::new);
        if devhooks::active() {
            devhooks::lap(&format!("{block:?} drawn in {:.1} ms", started.elapsed().as_secs_f64() * 1000.0));
        }
        self.imp().drawings.borrow_mut().insert(key, made.clone());
        made
    }

    fn make(&self, block: Block, source: &str) -> Option<Drawing> {
        let finder = self.imp().finder.borrow();
        let find = |path: &str| finder.as_ref().and_then(|find| find(path));
        let text = |text: &str| self.body_layout(text);
        match block {
            Block::Image => {
                let (texture, width, height) = render::picture(source, &find)?;
                Some(Drawing::Picture(texture.upcast(), width as f32, height as f32, false))
            }
            Block::Chart => {
                let fence = |line: &&str| line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~");
                let code: Vec<&str> = source.lines().skip(1).take_while(|line| !fence(line)).collect();
                let chart = flowchart::parse(&code.join("\n"))?;
                let paintable = flowchart::picture(&chart, &text).paintable()?;
                let (width, height) = (paintable.intrinsic_width() as f32, paintable.intrinsic_height() as f32);
                Some(Drawing::Picture(paintable, width, height, true))
            }
            Block::Math => {
                let latex = source.trim().strip_prefix("$$")?.strip_suffix("$$")?.trim();
                let layout = text("");
                if let Some(mut font) = layout.font_description() {
                    font.set_size((f64::from(font.size()) * 1.15) as i32);
                    layout.set_font_description(Some(&font));
                }
                layout.set_markup(&render::math_markup(latex, false));
                layout.set_alignment(pango::Alignment::Center);
                Some(Drawing::Math(layout))
            }
            Block::Table => {
                let table = render::table(source, &find, &text)?;
                let digit = text("0").pixel_size().0.max(1);
                let (mut cells, mut columns, mut rows) = (Vec::new(), Vec::<f32>::new(), Vec::new());
                for (header, row) in &table.rows {
                    let mut laid = Vec::new();
                    let mut tallest = 0f32;
                    for (column, cell) in row.iter().enumerate() {
                        let layout = text("");
                        layout.set_markup(&if *header { format!("<b>{}</b>", cell.markup) } else { cell.markup.clone() });
                        // Long cells wrap, as in the rendered view.
                        if render::display_width(&cell.plain) > 30 {
                            layout.set_width(30 * digit * pango::SCALE);
                            layout.set_wrap(pango::WrapMode::WordChar);
                        }
                        let (width, height) = layout.pixel_size();
                        let rtl = pango::find_base_dir(&cell.plain) == pango::Direction::Rtl;
                        let xalign = match table.aligns.get(column) {
                            Some(Alignment::Center) => 0.5,
                            Some(Alignment::Right) => 1.0,
                            Some(Alignment::Left) => 0.0,
                            _ if rtl => 1.0,
                            _ => 0.0,
                        };
                        if columns.len() <= column {
                            columns.push(0.0);
                        }
                        columns[column] = columns[column].max(width as f32);
                        tallest = tallest.max(height as f32);
                        laid.push((layout, xalign));
                    }
                    rows.push(tallest);
                    cells.push(laid);
                }
                Some(Drawing::Table(cells, columns, rows))
            }
        }
    }

    /// A layout for `text` in the rendered view's font, at this view's size.
    fn body_layout(&self, text: &str) -> pango::Layout {
        let layout = self.create_pango_layout(Some(text));
        let mut font = self.pango_context().font_description().unwrap_or_default();
        font.set_family(fonts::BODY);
        layout.set_font_description(Some(&font));
        layout
    }

    /// The checkbox drawn at `x`, `y` (in the view), as the range of its "[ ]".
    pub fn task_at(&self, x: f64, y: f64) -> Option<Range<usize>> {
        let (x, y) = self.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        let at = self.iter_at_location(x, y)?.offset() as usize;
        let marks = self.imp().marks.borrow();
        marks.iter().find(|(range, mark)| matches!(mark, Mark::Task(_)) && range.contains(&at)).map(|(range, _)| range.clone())
    }
}

impl Default for Paper {
    fn default() -> Self {
        Self::new()
    }
}

/// How far a callout's box reaches past its text, above and below.
const PAD: i32 = 6;

/// Room above and below a drawn block; a table's padding, inside its border, and the space
/// between its columns and rows (as the rendered view's).
const ABOVE: i32 = 4;
const BELOW: i32 = 10;
const CELL_SIDE: f32 = 10.0;
const CELL_TOP: f32 = 6.0;
const COLUMN_GAP: f32 = 18.0;
const ROW_GAP: f32 = 6.0;

/// What Live Preview draws for a block (see `style::blocks`).
pub enum Drawing {
    /// A picture or a flowchart, this big (smaller to fit), centred or not.
    Picture(gdk::Paintable, f32, f32, bool),
    /// A formula, centred.
    Math(pango::Layout),
    /// A table: its cells' text and where in its column each goes, the columns' widths, the
    /// rows' heights.
    Table(Vec<Vec<(pango::Layout, f32)>>, Vec<f32>, Vec<f32>),
}

impl Drawing {
    /// How big it is drawn `room` pixels wide.
    fn size(&self, room: f32) -> (f32, f32) {
        let gaps = |n: usize, gap: f32| n.saturating_sub(1) as f32 * gap;
        match self {
            Drawing::Picture(_, width, height, _) if *width > room => (room, height * room / width),
            Drawing::Picture(_, width, height, _) => (*width, *height),
            Drawing::Math(layout) => {
                let (width, height) = layout.pixel_size();
                (width as f32, height as f32)
            }
            Drawing::Table(_, columns, rows) => (
                2.0 + 2.0 * CELL_SIDE + columns.iter().sum::<f32>() + gaps(columns.len(), COLUMN_GAP),
                2.0 + 2.0 * CELL_TOP + rows.iter().sum::<f32>() + gaps(rows.len(), ROW_GAP),
            ),
        }
    }
}

/// `blocks` over their lines (and the room above them), those on screen: the lines covered in
/// the page's colour, the drawing in the middle.
fn draw_blocks(view: &Paper, blocks: &[(Range<usize>, Rc<Drawing>)], snapshot: &gtk::Snapshot) {
    let buffer = view.buffer();
    let seen = view.visible_rect();
    let left = view.left_margin() as f32;
    let room = (seen.width() - view.left_margin() - view.right_margin()) as f32;
    let colors = theme::colors();
    for (range, drawing) in blocks {
        let (y, _) = view.line_yrange(&buffer.iter_at_offset(range.start as i32));
        let (last, last_height) = view.line_yrange(&buffer.iter_at_offset(range.end as i32));
        let height = last + last_height - y;
        if y > seen.y() + seen.height() || y + height < seen.y() {
            continue;
        }
        let cover = graphene::Rect::new(seen.x() as f32, y as f32, seen.width() as f32, height as f32);
        snapshot.append_color(&render::rgba(colors.background, 1.0), &cover);
        let (width, tall) = drawing.size(room);
        let top = (y as f32 + (height as f32 - tall) / 2.0).round();
        match drawing.as_ref() {
            Drawing::Picture(paintable, _, _, centred) => {
                let x = if *centred { left + (room - width) / 2.0 } else { left };
                snapshot.save();
                snapshot.translate(&graphene::Point::new(x.round(), top));
                paintable.snapshot(snapshot, f64::from(width), f64::from(tall));
                snapshot.restore();
            }
            Drawing::Math(layout) => {
                snapshot.save();
                snapshot.translate(&graphene::Point::new((left + (room - width) / 2.0).round(), top));
                snapshot.append_layout(layout, &render::rgba(colors.text, 1.0));
                snapshot.restore();
            }
            Drawing::Table(cells, columns, rows) => {
                let frame = graphene::Rect::new(left, top, width.min(room), tall);
                let rounded = gsk::RoundedRect::from_rect(frame, 6.0);
                snapshot.push_clip(&frame);
                snapshot.push_rounded_clip(&rounded);
                snapshot.append_color(&render::rgba(colors.surface, 1.0), &frame);
                snapshot.pop();
                let mut row_top = top + 1.0 + CELL_TOP;
                for (row, laid) in cells.iter().enumerate() {
                    let mut column_left = left + 1.0 + CELL_SIDE;
                    for ((layout, xalign), column) in laid.iter().zip(columns) {
                        let x = column_left + (column - layout.pixel_size().0 as f32) * xalign;
                        snapshot.save();
                        snapshot.translate(&graphene::Point::new(x.round(), row_top.round()));
                        snapshot.append_layout(layout, &render::rgba(colors.text, 1.0));
                        snapshot.restore();
                        column_left += column + COLUMN_GAP;
                    }
                    row_top += rows[row] + ROW_GAP;
                }
                let border = render::rgba(colors.border, 1.0);
                snapshot.append_border(&rounded, &[1.0; 4], &[border; 4]);
                snapshot.pop();
            }
        }
    }
}

/// Where the character at `iter` is drawn: its left edge (its right one in right-to-left
/// text), and its line's top and height. GTK's `iter_location` places a character as if the
/// hidden text before it on its line were shown; the cursor's place leaves it out, as the
/// drawing does.
fn place(view: &Paper, iter: &gtk::TextIter) -> gdk::Rectangle {
    view.cursor_locations(Some(iter)).0
}

/// From the left edge of the character at `start` to the right edge of the one before `end`,
/// whichever way the line runs: x and width.
fn span(view: &Paper, start: &gtk::TextIter, end: &gtk::TextIter) -> (f32, f32) {
    let (a, b) = (place(view, start).x() as f32, place(view, end).x() as f32);
    (a.min(b), (a - b).abs())
}

/// `panels` behind the text, in buffer coordinates: those on screen, outer ones first.
fn draw_panels(view: &Paper, panels: &[Panel], snapshot: &gtk::Snapshot) {
    let buffer = view.buffer();
    let seen = view.visible_rect();
    let left = view.left_margin() as f32;
    let right = (seen.x() + seen.width() - view.right_margin()) as f32;
    for panel in panels {
        let first = buffer.iter_at_mark(&panel.start);
        // A folded callout is its header.
        let mut last = match &panel.fold {
            Some((tag, body)) if tag.is_invisible() => buffer.iter_at_mark(body),
            _ => buffer.iter_at_mark(&panel.end),
        };
        if last.starts_line() && last.offset() > first.offset() {
            last.backward_char();
        }
        // Inside a folded callout: the line ending the block is hidden too.
        if last.offset() <= first.offset() || last.tags().iter().any(|tag| tag.is_invisible()) {
            continue;
        }
        let pad = match panel.look {
            Look::Box(_) => PAD,
            Look::Bar(_) => 1,
        };
        let (top, bottom) = (place(view, &first), place(view, &last));
        let y = top.y() - pad;
        let height = bottom.y() + bottom.height() + pad - y;
        if y > seen.y() + seen.height() || y + height < seen.y() {
            continue;
        }
        let (start, end) = (left + panel.inset as f32, right - panel.inset as f32);
        match &panel.look {
            Look::Box(color) => {
                let rect = graphene::Rect::new(start, y as f32, end - start, height as f32);
                snapshot.push_rounded_clip(&gsk::RoundedRect::from_rect(rect, 6.0));
                snapshot.append_color(color, &rect);
                snapshot.pop();
            }
            Look::Bar(color) => {
                let x = if panel.rtl { end - 3.0 } else { start };
                snapshot.append_color(color, &graphene::Rect::new(x, y as f32, 3.0, height as f32));
            }
        }
    }
}

/// `marks` over the text, in buffer coordinates: those on screen.
fn draw_marks(view: &Paper, marks: &[(Range<usize>, Mark)], snapshot: &gtk::Snapshot) {
    if marks.is_empty() {
        return;
    }
    let buffer = view.buffer();
    let seen = view.visible_rect();
    let offset = |x: i32, y: i32| view.iter_at_location(x, y).map(|iter| iter.offset() as usize);
    let first = offset(seen.x(), seen.y()).unwrap_or(0);
    let last = offset(seen.x() + seen.width(), seen.y() + seen.height()).unwrap_or(usize::MAX);
    let from = marks.partition_point(|(range, _)| range.end < first);
    let colors = theme::colors();
    for (range, mark) in &marks[from..] {
        if range.start > last {
            break;
        }
        let start = buffer.iter_at_offset(range.start as i32);
        let at = place(view, &start);
        let (x, y, height) = (at.x() as f32, at.y() as f32, at.height() as f32);
        match mark {
            Mark::Bullet => {
                let mut next = start;
                next.forward_char();
                let (left, width) = span(view, &start, &next);
                let r = (height * 0.12).max(2.5);
                let (cx, cy) = (left + width / 2.0, y + height / 2.0);
                let dot = graphene::Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r);
                snapshot.push_rounded_clip(&gsk::RoundedRect::from_rect(dot, r));
                snapshot.append_color(&render::rgba(colors.bullet, 1.0), &dot);
                snapshot.pop();
            }
            Mark::Task(done) => {
                let (left, width) = span(view, &start, &buffer.iter_at_offset(range.end as i32));
                let side = (height * 0.7).min(width).round();
                let (left, top) = ((left + (width - side) / 2.0).round(), (y + (height - side) / 2.0).round());
                let square = graphene::Rect::new(left, top, side, side);
                let rounded = gsk::RoundedRect::from_rect(square, 3.0);
                if *done {
                    snapshot.push_rounded_clip(&rounded);
                    snapshot.append_color(&render::rgba(colors.accent, 1.0), &square);
                    snapshot.pop();
                    let tick = gsk::PathBuilder::new();
                    tick.move_to(left + side * 0.25, top + side * 0.52);
                    tick.line_to(left + side * 0.43, top + side * 0.70);
                    tick.line_to(left + side * 0.76, top + side * 0.32);
                    let stroke = gsk::Stroke::new(side * 0.13);
                    stroke.set_line_cap(gsk::LineCap::Round);
                    stroke.set_line_join(gsk::LineJoin::Round);
                    snapshot.append_stroke(&tick.to_path(), &stroke, &render::rgba(colors.background, 1.0));
                } else {
                    let color = render::rgba(colors.dim, 1.0);
                    snapshot.append_border(&rounded, &[1.5; 4], &[color; 4]);
                }
            }
            Mark::Rule => {
                let left = view.left_margin() as f32;
                let right = (seen.x() + seen.width() - view.right_margin()) as f32;
                let line = graphene::Rect::new(left, (y + height / 2.0).round(), right - left, 1.0);
                snapshot.append_color(&render::rgba(colors.rule, 1.0), &line);
            }
            Mark::Title(title, kind) => {
                let layout = view.create_pango_layout(Some(title));
                let bold = pango::AttrList::new();
                bold.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
                layout.set_attributes(Some(&bold));
                snapshot.save();
                snapshot.translate(&graphene::Point::new(x, y));
                snapshot.append_layout(&layout, &render::rgba(render::callout_color(kind), 1.0));
                snapshot.restore();
            }
        }
    }
}

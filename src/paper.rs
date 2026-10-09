//! The text views' paper: what nevis draws behind and over the text. Behind it, each block's
//! panel: a callout's box, a bar down a quote's or an embedded note's side. A panel spans its
//! whole block whatever the margins of its lines; a paragraph background starts at each line's
//! own margin, so a list in a callout would leave notches in it. Over the text, in Live
//! Preview: dots, checkboxes, rules and callout titles where the markup under them is.

use std::cell::RefCell;
use std::ops::Range;

use gtk::{gdk, glib, graphene, gsk, pango, prelude::*, subclass::prelude::*};
use sourceview::subclass::prelude::*;

use crate::render::{self, Look, Panel};
use crate::style::Mark;
use crate::theme;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Paper {
        pub panels: RefCell<Vec<Panel>>,
        pub marks: RefCell<Vec<(Range<usize>, Mark)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Paper {
        const NAME: &'static str = "NevisPaper";
        type Type = super::Paper;
        type ParentType = sourceview::View;
    }

    impl ObjectImpl for Paper {}
    impl WidgetImpl for Paper {}
    impl ViewImpl for Paper {}

    impl TextViewImpl for Paper {
        fn snapshot_layer(&self, layer: gtk::TextViewLayer, snapshot: gtk::Snapshot) {
            if layer == gtk::TextViewLayer::BelowText {
                super::draw_panels(&self.obj(), &self.panels.borrow(), &snapshot);
            }
            self.parent_snapshot_layer(layer, snapshot.clone());
            if layer == gtk::TextViewLayer::AboveText {
                super::draw_marks(&self.obj(), &self.marks.borrow(), &snapshot);
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

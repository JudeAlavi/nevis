//! The rendered view: a text view that also draws each block's panel behind its text, a
//! callout's box or a bar down a quote's or an embedded note's side. A panel spans its whole
//! block whatever the margins of its lines; a paragraph background starts at each line's own
//! margin, so a list in a callout would leave notches in it.

use std::cell::RefCell;

use gtk::{glib, graphene, gsk, prelude::*, subclass::prelude::*};
use sourceview::subclass::prelude::*;

use crate::render::{Look, Panel};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Reader {
        pub panels: RefCell<Vec<Panel>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Reader {
        const NAME: &'static str = "NevisReader";
        type Type = super::Reader;
        type ParentType = sourceview::View;
    }

    impl ObjectImpl for Reader {}
    impl WidgetImpl for Reader {}
    impl ViewImpl for Reader {}

    impl TextViewImpl for Reader {
        fn snapshot_layer(&self, layer: gtk::TextViewLayer, snapshot: gtk::Snapshot) {
            if layer == gtk::TextViewLayer::BelowText {
                super::draw(&self.obj(), &self.panels.borrow(), &snapshot);
            }
            self.parent_snapshot_layer(layer, snapshot);
        }
    }
}

glib::wrapper! {
    pub struct Reader(ObjectSubclass<imp::Reader>)
        @extends sourceview::View, gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl Reader {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Draws `panels`, marked in the buffer shown now, behind the text.
    pub fn set_panels(&self, panels: Vec<Panel>) {
        *self.imp().panels.borrow_mut() = panels;
        self.queue_draw();
    }
}

/// How far a callout's box reaches past its text, above and below.
const PAD: i32 = 6;

/// `panels` behind the text, in buffer coordinates: those on screen, outer ones first.
fn draw(view: &Reader, panels: &[Panel], snapshot: &gtk::Snapshot) {
    let buffer = view.buffer();
    let seen = view.visible_rect();
    let right = (seen.x() + seen.width()) as f32;
    for panel in panels {
        let first = buffer.iter_at_mark(&panel.start);
        // Inside a folded callout.
        if first.tags().iter().any(|tag| tag.is_invisible()) {
            continue;
        }
        // A folded callout is its header.
        let mut last = match &panel.fold {
            Some((tag, body)) if tag.is_invisible() => buffer.iter_at_mark(body),
            _ => buffer.iter_at_mark(&panel.end),
        };
        if last.starts_line() && last.offset() > first.offset() {
            last.backward_char();
        }
        if last.offset() <= first.offset() {
            continue;
        }
        let pad = match panel.look {
            Look::Box(_) => PAD,
            Look::Bar(_) => 1,
        };
        let (top, bottom) = (view.iter_location(&first), view.iter_location(&last));
        let y = top.y() - pad;
        let height = bottom.y() + bottom.height() + pad - y;
        if y > seen.y() + seen.height() || y + height < seen.y() {
            continue;
        }
        let inset = panel.inset as f32;
        match &panel.look {
            Look::Box(color) => {
                let rect = graphene::Rect::new(inset, y as f32, right - 2.0 * inset, height as f32);
                snapshot.push_rounded_clip(&gsk::RoundedRect::from_rect(rect, 6.0));
                snapshot.append_color(color, &rect);
                snapshot.pop();
            }
            Look::Bar(color) => {
                let x = if panel.rtl { right - inset - 3.0 } else { inset };
                snapshot.append_color(color, &graphene::Rect::new(x, y as f32, 3.0, height as f32));
            }
        }
    }
}

//! The folder sidebar (Ctrl+B): a folder's subfolders and text files as a tree.
//!
//! Its icons, arrows and indent guides are drawn here in the theme's colours rather than
//! taken from the icon theme, so showing the sidebar doesn't pay for GTK's icon loading
//! (~0.3 s the first time). The listing follows the disk: files appear and vanish as they change.

use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::{cairo, gdk, gio, glib, pango, prelude::*};

use crate::{devhooks, listing};

const ATTRIBUTES: &str = "standard::name,standard::display-name,standard::type,standard::is-hidden";
/// How far each level of the tree is indented, in pixels.
const INDENT: i32 = 14;
/// The room for a folder's arrow, after the indent.
const ARROW: i32 = 16;

pub struct Sidebar {
    pub widget: gtk::Box,
    selection: gtk::SingleSelection,
    /// The file to highlight once its row is listed (folders load in the background).
    wanted: Rc<RefCell<Option<gio::File>>>,
}

impl Sidebar {
    /// `open` is called with a file the user picked; `menu` with a row's file, whether it is
    /// a folder, and the row (to anchor a menu to) when the row is right-clicked.
    pub fn new(
        folder: &gio::File,
        open: impl Fn(gio::File) + 'static,
        menu: impl Fn(gio::File, bool, &gtk::Widget) + 'static,
    ) -> Self {
        let menu = Rc::new(menu);
        let (root, listing) = directory(folder);
        let tree = gtk::TreeListModel::new(root, false, false, |item| {
            let info = item.downcast_ref::<gio::FileInfo>()?;
            is_folder(info).then(|| file_of(info).map(|f| directory(&f).0.upcast()))?
        });
        let selection = gtk::SingleSelection::builder().model(&tree).autoselect(false).can_unselect(true).build();

        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
            // The indent guides and a folder's arrow, the icon, the name.
            let lead = gtk::DrawingArea::builder()
                .css_classes(["nevis-sidebar-arrow"])
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build();
            lead.set_draw_func(draw_lead);
            let icon = gtk::DrawingArea::builder()
                .content_width(16)
                .content_height(16)
                .valign(gtk::Align::Center)
                .css_classes(["nevis-file-icon"])
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build();
            icon.set_draw_func(draw_icon);
            let name = gtk::Label::builder()
                .xalign(0.0)
                .hexpand(true)
                .ellipsize(pango::EllipsizeMode::Middle)
                .css_classes(["nevis-sidebar-name"])
                .build();
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            row.append(&lead);
            row.append(&icon);
            row.append(&name);
            let click = gtk::GestureClick::builder().button(gdk::BUTTON_SECONDARY).build();
            let (menu, bound) = (menu.clone(), item.downgrade());
            click.connect_pressed(move |gesture, _, _, _| {
                let Some(item) = bound.upgrade() else { return };
                let info = item.item().and_downcast::<gtk::TreeListRow>().and_then(|row| row.item()).and_downcast::<gio::FileInfo>();
                if let (Some(info), Some(widget)) = (info, item.child())
                    && let Some(file) = file_of(&info)
                {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    menu(file, is_folder(&info), &widget);
                }
            });
            row.add_controller(click);
            item.set_child(Some(&row));
        });
        // Each folder row's arrow and icon follow its expanded state while the row is bound.
        let watched: Rc<RefCell<HashMap<gtk::ListItem, (gtk::TreeListRow, glib::SignalHandlerId)>>> = Default::default();
        let bound = watched.clone();
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
            let (Some(row), Some(widget)) =
                (item.item().and_downcast::<gtk::TreeListRow>(), item.child().and_downcast::<gtk::Box>())
            else {
                return;
            };
            let Some(info) = row.item().and_downcast::<gio::FileInfo>() else { return };
            let lead = widget.first_child().and_downcast::<gtk::DrawingArea>().expect("indent and arrow");
            let icon = lead.next_sibling().and_downcast::<gtk::DrawingArea>().expect("icon");
            let label = icon.next_sibling().and_downcast::<gtk::Label>().expect("name label");
            lead.set_content_width(INDENT * row.depth() as i32 + ARROW);
            label.set_text(&shown_name(&info));
            // What the drawing reads: the kind of entry, and whether a folder is open.
            let kind = if is_folder(&info) {
                "folder"
            } else if listing::is_note(&name(&info)) {
                "note"
            } else {
                "text"
            };
            for area in [&lead, &icon] {
                for old in ["folder", "note", "text"] {
                    area.remove_css_class(old);
                }
                area.add_css_class(kind);
            }
            show_open(&lead, &icon, row.is_expanded());
            if is_folder(&info) {
                let (lead, icon) = (lead.clone(), icon.clone());
                let handler = row.connect_expanded_notify(move |row| show_open(&lead, &icon, row.is_expanded()));
                bound.borrow_mut().insert(item.clone(), (row, handler));
            }
        });
        factory.connect_unbind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
            if let Some((row, handler)) = watched.borrow_mut().remove(item) {
                row.disconnect(handler);
            }
        });

        let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
        list.add_css_class("nevis-sidebar");
        list.set_single_click_activate(true);
        list.connect_activate(move |list, position| {
            let Some(row) = list.model().and_then(|m| m.item(position)).and_downcast::<gtk::TreeListRow>() else {
                return;
            };
            let Some(info) = row.item().and_downcast::<gio::FileInfo>() else { return };
            if is_folder(&info) {
                row.set_expanded(!row.is_expanded());
            } else if let Some(file) = file_of(&info) {
                open(file);
            }
        });

        // For NEVIS_TRACE: when the folder is listed, and when that listing is on screen.
        let shown = list.downgrade();
        listing.connect_loading_notify(move |listing| {
            if listing.is_loading() {
                return;
            }
            devhooks::lap("sidebar listed");
            if let Some(window) = shown.upgrade().and_then(|list| list.root()).and_downcast::<gtk::ApplicationWindow>() {
                devhooks::on_next_paint(&window, || devhooks::lap("sidebar painted"));
            }
        });

        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        // Above the tree: the folder's name, and "+" for a new note (a label: no icons).
        let title = folder.basename().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let title = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(pango::EllipsizeMode::End)
            .css_classes(["nevis-sidebar-title"])
            .build();
        let new_note = gtk::Label::builder().label("+").css_classes(["nevis-sidebar-new"]).tooltip_text("New note (Ctrl+N)").build();
        let click = gtk::GestureClick::new();
        click.connect_released(|click, _, _, _| {
            if let Some(widget) = click.widget() {
                let _ = widget.activate_action("win.new-note", None);
            }
        });
        new_note.add_controller(click);
        let header = gtk::Box::builder().css_classes(["nevis-sidebar-header"]).build();
        header.append(&title);
        header.append(&new_note);
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["nevis-sidebar-pane"])
            .width_request(180)
            .build();
        widget.append(&header);
        widget.append(&scroller);
        let wanted: Rc<RefCell<Option<gio::File>>> = Default::default();
        let (pending, rows) = (wanted.clone(), selection.clone());
        let opening = Rc::new(std::cell::Cell::new(false));
        tree.connect_items_changed(move |_, _, _, _| {
            let file = pending.borrow().clone();
            let Some(file) = file else { return };
            if select_in(&rows, &file) {
                pending.borrow_mut().take();
            } else if !opening.replace(true) {
                // The next folder down to it, once this change is over.
                let (rows, opening) = (rows.clone(), opening.clone());
                glib::idle_add_local_once(move || {
                    opening.set(false);
                    reveal(&rows, &file);
                });
            }
        });
        Self { widget, selection, wanted }
    }

    /// Highlights `file`'s row: now if it is listed, or as soon as its folder's listing
    /// shows it, opening the folders down to it.
    pub fn select(&self, file: &gio::File) {
        if select_in(&self.selection, file) {
            self.wanted.borrow_mut().take();
        } else {
            self.selection.set_selected(gtk::INVALID_LIST_POSITION);
            *self.wanted.borrow_mut() = Some(file.clone());
            reveal(&self.selection, file);
        }
    }
}

/// Opens the deepest folder listed so far that holds `file` (its listing then shows the
/// next one down, or the file).
fn reveal(selection: &gtk::SingleSelection, file: &gio::File) {
    let Some(model) = selection.model() else { return };
    let holder = (0..model.n_items())
        .filter_map(|i| model.item(i).and_downcast::<gtk::TreeListRow>())
        .filter(|row| {
            !row.is_expanded()
                && row
                    .item()
                    .and_downcast::<gio::FileInfo>()
                    .is_some_and(|info| is_folder(&info) && file_of(&info).is_some_and(|folder| file.has_prefix(&folder)))
        })
        .max_by_key(|row| row.depth());
    if let Some(row) = holder {
        row.set_expanded(true);
    }
}

/// Selects `file`'s row if it is listed; whether it was.
fn select_in(selection: &gtk::SingleSelection, file: &gio::File) -> bool {
    let Some(model) = selection.model() else { return false };
    let position = (0..model.n_items()).find(|&i| {
        model
            .item(i)
            .and_downcast::<gtk::TreeListRow>()
            .and_then(|row| row.item().and_downcast::<gio::FileInfo>())
            .and_then(|info| file_of(&info))
            .is_some_and(|listed| listed.equal(file))
    });
    if let Some(position) = position {
        selection.set_selected(position);
    }
    position.is_some()
}

/// One folder's entries, filtered and sorted for the sidebar, and the listing they come
/// from; it follows changes on disk.
fn directory(folder: &gio::File) -> (gtk::SortListModel, gtk::DirectoryList) {
    let list = gtk::DirectoryList::new(Some(ATTRIBUTES), Some(folder));
    let filter = gtk::CustomFilter::new(|item| {
        item.downcast_ref::<gio::FileInfo>()
            .is_some_and(|info| listing::shown(&name(info), is_folder(info), info.is_hidden()))
    });
    let sorter = gtk::CustomSorter::new(|a, b| {
        let (Some(a), Some(b)) = (a.downcast_ref::<gio::FileInfo>(), b.downcast_ref::<gio::FileInfo>()) else {
            return gtk::Ordering::Equal;
        };
        listing::order((&name(a), is_folder(a)), (&name(b), is_folder(b))).into()
    });
    let filtered = gtk::FilterListModel::new(Some(list.clone()), Some(filter));
    (gtk::SortListModel::new(Some(filtered), Some(sorter)), list)
}

fn name(info: &gio::FileInfo) -> String {
    info.name().to_string_lossy().into_owned()
}

/// Marks a folder row's arrow and icon open or closed.
fn show_open(lead: &gtk::DrawingArea, icon: &gtk::DrawingArea, open: bool) {
    for area in [lead, icon] {
        if open {
            area.add_css_class("open");
        } else {
            area.remove_css_class("open");
        }
        area.queue_draw();
    }
}

/// Sets `cr` to draw in the widget's text colour, at `alpha` of its opacity.
fn ink(area: &gtk::DrawingArea, cr: &cairo::Context, alpha: f64) {
    let color = area.color();
    cr.set_source_rgba(color.red().into(), color.green().into(), color.blue().into(), f64::from(color.alpha()) * alpha);
}

/// Before a row's icon: a guide line for each level above it, and a folder's arrow.
fn draw_lead(area: &gtk::DrawingArea, cr: &cairo::Context, width: i32, height: i32) {
    let depth = (width - ARROW) / INDENT;
    cr.set_line_width(1.0);
    ink(area, cr, 0.28);
    for level in 0..depth {
        // Under the arrow of the folder at that level; .5 keeps a 1 px line sharp.
        let x = f64::from(level * INDENT + ARROW / 2) + 0.5;
        cr.move_to(x, 0.0);
        cr.line_to(x, height.into());
    }
    let _ = cr.stroke();
    if !area.has_css_class("folder") {
        return;
    }
    let (x, y) = (f64::from(depth * INDENT + ARROW / 2), f64::from(height) / 2.0);
    ink(area, cr, 0.9);
    cr.set_line_width(1.4);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    if area.has_css_class("open") {
        cr.move_to(x - 3.5, y - 1.75);
        cr.line_to(x, y + 1.75);
        cr.line_to(x + 3.5, y - 1.75);
    } else {
        cr.move_to(x - 1.75, y - 3.5);
        cr.line_to(x + 1.75, y);
        cr.line_to(x - 1.75, y + 3.5);
    }
    let _ = cr.stroke();
}

/// A 16 px icon in the widget's colour: a folder (open or closed), or a page; a note's page
/// starts with a heading.
fn draw_icon(area: &gtk::DrawingArea, cr: &cairo::Context, _width: i32, _height: i32) {
    cr.set_line_width(1.25);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    // A shape filled faintly, then outlined.
    let shade = |fill: f64| {
        ink(area, cr, fill);
        let _ = cr.fill_preserve();
        ink(area, cr, 1.0);
        let _ = cr.stroke();
    };
    if area.has_css_class("folder") {
        // The back, with its tab.
        cr.move_to(1.5, 13.5);
        cr.line_to(1.5, 3.9);
        cr.arc(2.9, 3.9, 1.4, PI, 1.5 * PI);
        cr.line_to(5.6, 2.5);
        cr.line_to(7.1, 4.0);
        cr.line_to(13.0, 4.0);
        cr.arc(13.0, 5.5, 1.5, 1.5 * PI, 2.0 * PI);
        if area.has_css_class("open") {
            cr.line_to(14.5, 7.0);
            ink(area, cr, 1.0);
            let _ = cr.stroke();
            // The front, tipped forward.
            cr.move_to(1.5, 13.5);
            cr.line_to(3.9, 7.0);
            cr.line_to(15.2, 7.0);
            cr.line_to(12.8, 13.5);
            cr.close_path();
            shade(0.3);
        } else {
            cr.line_to(14.5, 12.0);
            cr.arc(13.0, 12.0, 1.5, 0.0, 0.5 * PI);
            cr.line_to(3.0, 13.5);
            cr.arc(3.0, 12.0, 1.5, 0.5 * PI, PI);
            cr.close_path();
            shade(0.22);
        }
        return;
    }
    // A page with its corner folded.
    cr.move_to(4.5, 1.5);
    cr.line_to(9.5, 1.5);
    cr.line_to(13.0, 5.0);
    cr.line_to(13.0, 13.0);
    cr.arc(11.5, 13.0, 1.5, 0.0, 0.5 * PI);
    cr.line_to(4.5, 14.5);
    cr.arc(4.5, 13.0, 1.5, 0.5 * PI, PI);
    cr.line_to(3.0, 3.0);
    cr.arc(4.5, 3.0, 1.5, PI, 1.5 * PI);
    cr.close_path();
    shade(0.1);
    cr.move_to(9.5, 1.5);
    cr.line_to(9.5, 5.0);
    cr.line_to(13.0, 5.0);
    let _ = cr.stroke();
    let heading = area.has_css_class("note");
    cr.set_line_width(if heading { 1.7 } else { 1.1 });
    cr.move_to(5.6, 8.4);
    cr.line_to(if heading { 8.8 } else { 10.4 }, 8.4);
    let _ = cr.stroke();
    cr.set_line_width(1.1);
    cr.move_to(5.6, 11.2);
    cr.line_to(10.4, 11.2);
    let _ = cr.stroke();
}

/// What the sidebar shows: the display name, without ".md" (every note has it).
fn shown_name(info: &gio::FileInfo) -> String {
    let display = info.display_name().to_string();
    match display.len().checked_sub(3) {
        Some(cut) if !is_folder(info) && display.is_char_boundary(cut) && display[cut..].eq_ignore_ascii_case(".md") => {
            display[..cut].to_owned()
        }
        _ => display,
    }
}

fn is_folder(info: &gio::FileInfo) -> bool {
    info.file_type() == gio::FileType::Directory
}

/// The file a directory listing entry stands for.
fn file_of(info: &gio::FileInfo) -> Option<gio::File> {
    info.attribute_object("standard::file").and_downcast::<gio::File>()
}

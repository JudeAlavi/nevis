//! One window = one file: its buffer, loading, saving and closing.
//!
//! The window has no header bar or icons (GTK's first icon lookup costs ~300 ms on startup);
//! the title goes to the compositor, and the menu lives in the right-click menu.
//! Markdown files open rendered (read mode); Ctrl+E or a double-click switches to the source.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::ops::Range;
use std::rc::Rc;

use gtk::{gdk, gio, glib, prelude::*};
use sourceview::prelude::*;

use crate::render::{self, Target};
use crate::{devhooks, find::FindBar, help, theme};

thread_local! {
    /// Owns every open editor; an entry is dropped when its window leaves the app.
    static EDITORS: RefCell<Vec<Rc<Editor>>> = const { RefCell::new(Vec::new()) };
}

pub struct Editor {
    window: gtk::ApplicationWindow,
    /// Holds `content`, with the shortcut cheat sheet drawn over it.
    overlay: gtk::Overlay,
    content: gtk::Box,
    /// Holds the source editor ("edit") and the rendered view ("read").
    stack: gtk::Stack,
    view: sourceview::View,
    buffer: sourceview::Buffer,
    reader: sourceview::View,
    /// Location, encoding and line endings of the file on disk.
    file: sourceview::File,
    /// Created on first Ctrl+F.
    find: RefCell<Option<Rc<FindBar>>>,
    /// The cheat sheet, created on first Ctrl+?.
    help: RefCell<Option<gtk::Widget>>,
    /// Shown in the title after the file name, e.g. "new file".
    note: RefCell<Option<String>>,
    /// Clickable links and checkboxes in the rendered view.
    targets: RefCell<Vec<(gtk::TextTag, Target)>>,
    /// The rendered view is up to date with the source.
    rendered: Cell<bool>,
    /// The user already confirmed closing; don't ask again.
    closing: Cell<bool>,
    /// The "save changes?" dialog is on screen.
    asking: Cell<bool>,
    saving: Cell<bool>,
}

impl Editor {
    /// Brings `file` to the front if it is already open, otherwise opens it in a new window.
    pub fn open(app: &gtk::Application, file: Option<gio::File>) {
        if let Some(existing) = file.as_ref().and_then(find_editor) {
            existing.window.present();
            return;
        }
        let editor = Editor::new(app);
        devhooks::lap("window built");
        editor.window.present();
        devhooks::lap("window presented");
        match file {
            Some(file) => editor.load(file),
            None => devhooks::content_ready(&editor.window),
        }
    }

    fn new(app: &gtk::Application) -> Rc<Self> {
        let buffer = sourceview::Buffer::new(None);
        buffer.set_highlight_matching_brackets(false);
        buffer.set_style_scheme(theme::scheme().as_ref());

        let view = sourceview::View::with_buffer(&buffer);
        view.add_css_class("nevis-editor");
        view.set_wrap_mode(gtk::WrapMode::WordChar);
        view.set_left_margin(16);
        view.set_right_margin(16);
        view.set_top_margin(10);
        view.set_bottom_margin(10);
        view.set_pixels_above_lines(2);
        view.set_pixels_below_lines(2);
        view.set_pixels_inside_wrap(2);
        view.set_highlight_current_line(true);
        view.set_auto_indent(true);
        view.set_insert_spaces_instead_of_tabs(true);
        view.set_tab_width(4);
        view.set_smart_backspace(true);
        view.set_show_line_numbers(true);
        view.set_extra_menu(Some(&menu_model()));

        let reader = sourceview::View::new();
        reader.add_css_class("nevis-read");
        reader.set_editable(false);
        reader.set_cursor_visible(false);
        reader.set_wrap_mode(gtk::WrapMode::WordChar);
        reader.set_left_margin(render::MARGIN);
        reader.set_right_margin(render::MARGIN);
        reader.set_top_margin(18);
        reader.set_bottom_margin(28);
        reader.set_pixels_inside_wrap(3);
        reader.set_extra_menu(Some(&menu_model()));

        let edit_scroller = gtk::ScrolledWindow::builder().child(&view).build();
        let read_scroller = gtk::ScrolledWindow::builder().child(&reader).build();
        let stack = gtk::Stack::builder().vexpand(true).hhomogeneous(false).vhomogeneous(false).build();
        stack.add_named(&edit_scroller, Some("edit"));
        stack.add_named(&read_scroller, Some("read"));
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&stack);
        let overlay = gtk::Overlay::builder().child(&content).build();

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .default_width(860)
            .default_height(640)
            .child(&overlay)
            .build();

        let editor = Rc::new(Self {
            window,
            overlay,
            content,
            stack,
            view,
            buffer,
            reader,
            file: sourceview::File::new(),
            find: RefCell::new(None),
            help: RefCell::new(None),
            note: RefCell::new(None),
            targets: RefCell::new(Vec::new()),
            rendered: Cell::new(false),
            closing: Cell::new(false),
            asking: Cell::new(false),
            saving: Cell::new(false),
        });
        editor.install_actions();
        editor.connect_signals();
        zoom_on_ctrl_scroll(&edit_scroller);
        zoom_on_ctrl_scroll(&read_scroller);
        editor.update_title();
        EDITORS.with_borrow_mut(|editors| editors.push(editor.clone()));
        editor
    }

    fn install_actions(self: &Rc<Self>) {
        self.action("open", |e| spawn(e.clone().open_dialog()));
        self.action("save", |e| spawn(e.clone().save()));
        self.action("save-as", |e| spawn(e.clone().save_as()));
        self.action("close", |e| e.window.close());
        self.action("find", |e| {
            e.set_reading(false);
            e.find_bar().show(false);
        });
        self.action("replace", |e| {
            e.set_reading(false);
            e.find_bar().show(true);
        });
        self.action("find-next", |e| {
            e.set_reading(false);
            e.find_bar().next();
        });
        self.action("find-prev", |e| {
            e.set_reading(false);
            e.find_bar().prev();
        });
        self.action("zoom-in", |_| theme::zoom(1.0));
        self.action("zoom-out", |_| theme::zoom(-1.0));
        self.action("zoom-reset", |_| theme::zoom(0.0));
        self.toggle("reading", false, |e, on| e.set_reading(on));
        self.toggle("wrap", true, |e, on| {
            e.view.set_wrap_mode(if on { gtk::WrapMode::WordChar } else { gtk::WrapMode::None });
        });
        self.toggle("line-numbers", true, |e, on| e.view.set_show_line_numbers(on));
        self.action("shortcuts", |e| e.toggle_help());
    }

    fn action(self: &Rc<Self>, name: &str, run: impl Fn(&Rc<Self>) + 'static) {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(self);
        action.connect_activate(move |_, _| {
            if let Some(editor) = weak.upgrade() {
                run(&editor);
            }
        });
        self.window.add_action(&action);
    }

    fn toggle(self: &Rc<Self>, name: &str, initial: bool, run: impl Fn(&Rc<Self>, bool) + 'static) {
        let action = gio::SimpleAction::new_stateful(name, None, &initial.to_variant());
        let weak = Rc::downgrade(self);
        action.connect_activate(move |action, _| {
            let on = !action.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
            action.set_state(&on.to_variant());
            if let Some(editor) = weak.upgrade() {
                run(&editor, on);
            }
        });
        self.window.add_action(&action);
    }

    fn connect_signals(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.buffer.connect_modified_changed(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.update_title();
            }
        });

        let weak = Rc::downgrade(self);
        self.buffer.connect_changed(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.rendered.set(false);
            }
        });

        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            let Some(editor) = weak.upgrade() else { return glib::Propagation::Proceed };
            if editor.closing.get() || !editor.buffer.is_modified() {
                return glib::Propagation::Proceed;
            }
            if !editor.asking.get() {
                spawn(editor.clone().confirm_close());
            }
            glib::Propagation::Stop
        });

        // Rendered view: click a link or checkbox to follow it, double-click to edit.
        let click = gtk::GestureClick::new();
        click.set_button(gdk::BUTTON_PRIMARY);
        let weak = Rc::downgrade(self);
        click.connect_released(move |_, presses, x, y| {
            let Some(editor) = weak.upgrade() else { return };
            if presses == 2 {
                editor.set_reading(false);
            } else if !editor.reader.buffer().has_selection()
                && let Some(target) = editor.target_at(x, y) {
                    editor.follow(target);
                }
        });
        self.reader.add_controller(click);

        // While the cheat sheet is open, a key closes it instead of reaching the text.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(editor) = weak.upgrade() else { return glib::Propagation::Proceed };
            if !editor.help_open() || is_modifier(key) {
                return glib::Propagation::Proceed;
            }
            editor.hide_help();
            glib::Propagation::Stop
        });
        self.window.add_controller(keys);

        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(self);
        motion.connect_motion(move |_, x, y| {
            if let Some(editor) = weak.upgrade() {
                let cursor = editor.target_at(x, y).map(|_| "pointer");
                editor.reader.set_cursor_from_name(cursor);
            }
        });
        self.reader.add_controller(motion);
    }

    fn find_bar(&self) -> Rc<FindBar> {
        self.find
            .borrow_mut()
            .get_or_insert_with(|| {
                let bar = FindBar::new(&self.view, &self.buffer);
                self.content.prepend(&bar.widget);
                bar
            })
            .clone()
    }

    fn toggle_help(self: &Rc<Self>) {
        if self.help_open() {
            return self.hide_help();
        }
        if self.help.borrow().is_none() {
            let Some(app) = self.window.application() else { return };
            let weak = Rc::downgrade(self);
            let sheet = help::build(&app, move || {
                if let Some(editor) = weak.upgrade() {
                    editor.hide_help();
                }
            });
            self.overlay.add_overlay(&sheet);
            *self.help.borrow_mut() = Some(sheet);
        }
        if let Some(sheet) = self.help.borrow().as_ref() {
            sheet.set_visible(true);
        }
    }

    fn help_open(&self) -> bool {
        self.help.borrow().as_ref().is_some_and(|sheet| sheet.is_visible())
    }

    fn hide_help(&self) {
        if let Some(sheet) = self.help.borrow().as_ref() {
            sheet.set_visible(false);
        }
        if self.stack.visible_child_name().as_deref() == Some("read") {
            self.reader.grab_focus();
        } else {
            self.view.grab_focus();
        }
    }

    /// Shows the rendered Markdown (true) or the source (false).
    fn set_reading(&self, on: bool) {
        if on && !self.rendered.get() {
            self.render_markdown();
        }
        self.stack.set_visible_child_name(if on { "read" } else { "edit" });
        if let Some(action) = self.window.lookup_action("reading").and_downcast::<gio::SimpleAction>() {
            action.set_state(&on.to_variant());
        }
        if on {
            self.reader.grab_focus();
        } else {
            self.view.grab_focus();
        }
    }

    fn render_markdown(&self) {
        let text = self.buffer.text(&self.buffer.start_iter(), &self.buffer.end_iter(), false);
        let folder = self.file.location().and_then(|l| l.parent());
        let rendered = render::render(&text, folder.as_ref(), theme::scheme().as_ref());
        self.reader.set_buffer(Some(&rendered.buffer));
        for (anchor, widget) in &rendered.children {
            self.reader.add_child_at_anchor(widget, anchor);
        }
        *self.targets.borrow_mut() = rendered.targets;
        self.rendered.set(true);
        devhooks::lap("markdown rendered");
    }

    fn target_at(&self, x: f64, y: f64) -> Option<Target> {
        let (bx, by) = self.reader.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        let iter = self.reader.iter_at_location(bx, by)?;
        let targets = self.targets.borrow();
        targets.iter().find(|(tag, _)| iter.has_tag(tag)).map(|(_, target)| target.clone())
    }

    fn follow(self: &Rc<Self>, target: Target) {
        match target {
            Target::Task(range) => self.toggle_task(range),
            Target::Link(url) => self.open_link(&url),
        }
    }

    /// Flips a `[ ]` / `[x]` checkbox in the source and re-renders in place.
    fn toggle_task(&self, marker: Range<usize>) {
        let text = self.buffer.text(&self.buffer.start_iter(), &self.buffer.end_iter(), false);
        let Some(open) = text.get(marker.clone()).and_then(|m| m.find('[')) else { return };
        let at = marker.start + open + 1;
        let Some(mark) = text[at..].chars().next() else { return };
        let offset = text[..at].chars().count() as i32;
        let mut start = self.buffer.iter_at_offset(offset);
        let mut end = self.buffer.iter_at_offset(offset + 1);
        self.buffer.begin_user_action();
        self.buffer.delete(&mut start, &mut end);
        self.buffer.insert(&mut start, if mark == ' ' { "x" } else { " " });
        self.buffer.end_user_action();

        // Re-rendering replaces the buffer, which scrolls to the top; put the scroll back.
        let scroll = self.reader.vadjustment().map(|a| a.value());
        self.render_markdown();
        if let (Some(value), Some(adjustment)) = (scroll, self.reader.vadjustment()) {
            after_next_frame(&self.window, move || adjustment.set_value(value));
        }
    }

    fn open_link(self: &Rc<Self>, url: &str) {
        if url.starts_with('#') {
            return; // jumps within the page aren't supported yet
        }
        let has_scheme = url.split_once(':').is_some_and(|(scheme, _)| {
            scheme.len() > 1 && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
        });
        if has_scheme {
            return launch(&self.window, url);
        }
        // A path relative to this file, or a [[wiki]] page name.
        let path = glib::Uri::unescape_string(url, None::<&str>).map_or_else(|| url.to_owned(), |p| p.to_string());
        let folder = self.file.location().and_then(|l| l.parent());
        let mut file = match &folder {
            Some(folder) => folder.resolve_relative_path(&path),
            None => gio::File::for_path(&path),
        };
        if file.basename().and_then(|n| n.extension().map(|e| e.to_owned())).is_none()
            && let Some(p) = file.path() {
                file = gio::File::for_path(format!("{}.md", p.display()));
            }
        if is_text(&file) {
            if let Some(app) = self.window.application() {
                Editor::open(&app, Some(file));
            }
        } else {
            launch(&self.window, &file.uri());
        }
    }

    fn load(self: &Rc<Self>, location: gio::File) {
        self.file.set_location(Some(&location));
        self.update_title();
        self.view.set_editable(false);
        let loader = sourceview::FileLoader::new(&self.buffer, &self.file);
        let weak = Rc::downgrade(self);
        spawn(async move {
            let (load, _progress) = loader.load_future(glib::Priority::DEFAULT);
            let result = load.await;
            devhooks::lap("file loaded");
            if let Some(editor) = weak.upgrade() {
                editor.loaded(result);
            }
        });
    }

    fn loaded(self: &Rc<Self>, result: Result<(), glib::Error>) {
        let mut editable = true;
        let note = match result {
            Ok(()) => None,
            Err(err) if err.matches(gio::IOErrorEnum::NotFound) => Some("new file".to_owned()),
            Err(err) if err.matches(sourceview::FileLoaderError::ConversionFallback) => {
                // Not text in any encoding we know; editing and saving would corrupt it.
                editable = false;
                Some("read-only: not valid text".to_owned())
            }
            Err(err) => {
                self.file.set_location(None::<&gio::File>);
                alert(&self.window, "Could not open the file", err.message());
                None
            }
        };
        let existing = note.is_none();
        *self.note.borrow_mut() = note;
        self.view.set_editable(editable);
        self.buffer.place_cursor(&self.buffer.start_iter());
        self.buffer.set_modified(false);
        self.update_title();
        if existing && self.file.location().is_some_and(|l| is_markdown(&l)) {
            self.set_reading(true);
        }
        devhooks::content_ready(&self.window);

        // Loading the highlighting rules takes ~100 ms; show the text first.
        let weak = Rc::downgrade(self);
        after_next_frame(&self.window, move || {
            if let Some(editor) = weak.upgrade()
                && let Some(location) = editor.file.location()
            {
                editor.set_language(&location);
                devhooks::lap("highlighting applied");
            }
        });
    }

    fn set_language(&self, location: &gio::File) {
        let manager = sourceview::LanguageManager::default();
        let name = location.basename();
        let language = if is_markdown(location) {
            manager.language("markdown")
        } else if extension(location).is_some_and(|e| e == "txt" || e == "text") {
            None
        } else {
            manager.guess_language(name.as_deref(), None)
        };
        self.buffer.set_language(language.as_ref());
    }

    async fn open_dialog(self: Rc<Self>) {
        let dialog = gtk::FileDialog::builder().title("Open").modal(true).build();
        if let Some(folder) = self.file.location().and_then(|l| l.parent()) {
            dialog.set_initial_folder(Some(&folder));
        }
        let Ok(file) = dialog.open_future(Some(&self.window)).await else { return };
        if let Some(existing) = find_editor(&file) {
            existing.window.present();
        } else if self.is_pristine() {
            self.load(file);
        } else if let Some(app) = self.window.application() {
            Editor::open(&app, Some(file));
        }
    }

    /// Saves to the current file, asking for a name first if there is none. True if saved.
    async fn save(self: Rc<Self>) -> bool {
        if self.file.location().is_none() {
            return self.save_as().await;
        }
        self.write().await
    }

    async fn save_as(self: Rc<Self>) -> bool {
        let dialog = gtk::FileDialog::builder().title("Save As").modal(true).build();
        match self.file.location() {
            Some(location) => dialog.set_initial_file(Some(&location)),
            None => dialog.set_initial_name(Some("Untitled.md")),
        }
        let Ok(location) = dialog.save_future(Some(&self.window)).await else { return false };
        self.file.set_location(Some(&location));
        self.set_language(&location);
        *self.note.borrow_mut() = None;
        self.update_title();
        self.write().await
    }

    async fn write(&self) -> bool {
        if !self.view.is_editable() || self.saving.replace(true) {
            return false;
        }
        let saver = sourceview::FileSaver::new(&self.buffer, &self.file);
        // A saver may be reconfigured and rerun after an error.
        let saved = loop {
            let (save, _progress) = saver.save_future(glib::Priority::DEFAULT);
            match save.await {
                Ok(()) => break true,
                Err(err) if err.matches(sourceview::FileSaverError::ExternallyModified) => {
                    let detail = "Saving will overwrite the changes made outside nevis.";
                    if choose(&self.window, "The file changed on disk", detail, &["Cancel", "Overwrite"]).await != Some(1) {
                        break false;
                    }
                    saver.set_flags(saver.flags() | sourceview::FileSaverFlags::IGNORE_MODIFICATION_TIME);
                }
                Err(err) if err.matches(sourceview::FileSaverError::InvalidChars) => {
                    let detail = "Some characters can't be stored in this file's encoding.";
                    if choose(&self.window, "Save as UTF-8?", detail, &["Cancel", "Save as UTF-8"]).await != Some(1) {
                        break false;
                    }
                    saver.set_encoding(Some(&sourceview::Encoding::utf8()));
                }
                Err(err) => {
                    alert(&self.window, "Could not save the file", err.message());
                    break false;
                }
            }
        };
        self.saving.set(false);
        if saved {
            self.buffer.set_modified(false);
            *self.note.borrow_mut() = None;
            self.update_title();
        }
        saved
    }

    async fn confirm_close(self: Rc<Self>) {
        self.asking.set(true);
        let message = format!("Save changes to “{}”?", self.display_name());
        let detail = "Your changes will be lost if you don't save them.";
        let choice = choose(&self.window, &message, detail, &["Cancel", "Don't Save", "Save"]).await;
        self.asking.set(false);
        let close = match choice {
            Some(2) => self.clone().save().await,
            Some(1) => true,
            _ => false,
        };
        if close {
            self.closing.set(true);
            self.window.close();
        }
    }

    fn is_pristine(&self) -> bool {
        self.file.location().is_none() && !self.buffer.is_modified() && self.buffer.char_count() == 0
    }

    fn display_name(&self) -> String {
        self.file
            .location()
            .and_then(|l| l.basename())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned())
    }

    /// "• notes.md (new file) — ~/Documents"
    fn update_title(&self) {
        let mut title = self.display_name();
        if self.buffer.is_modified() {
            title.insert_str(0, "• ");
        }
        if let Some(note) = self.note.borrow().as_deref() {
            title.push_str(&format!(" ({note})"));
        }
        if let Some(folder) = self.file.location().and_then(|l| l.parent()) {
            title.push_str(&format!(" — {}", tilde(&folder.parse_name())));
        }
        self.window.set_title(Some(&title));
    }
}

/// Drops the editor of a window that left the app (it was closed).
///
/// Not done on "destroy": GTK only emits that once the last reference is gone,
/// and the editor itself holds one.
pub fn forget(window: &gtk::Window) {
    let gone = EDITORS.with_borrow_mut(|editors| {
        let index = editors.iter().position(|e| e.window.upcast_ref::<gtk::Window>() == window)?;
        Some(editors.remove(index))
    });
    drop(gone); // outside the borrow: dropping widgets may run handlers
}

fn find_editor(file: &gio::File) -> Option<Rc<Editor>> {
    EDITORS.with_borrow(|editors| {
        editors
            .iter()
            .find(|e| e.file.location().is_some_and(|l| l.equal(file)))
            .cloned()
    })
}

fn extension(file: &gio::File) -> Option<String> {
    let name = file.basename()?;
    Some(name.extension()?.to_str()?.to_ascii_lowercase())
}

fn is_markdown(file: &gio::File) -> bool {
    extension(file).is_some_and(|e| matches!(e.as_str(), "md" | "markdown" | "mdown" | "mkd" | "mkdn"))
}

fn is_text(file: &gio::File) -> bool {
    is_markdown(file) || extension(file).is_some_and(|e| e == "txt" || e == "text")
}

/// Shift, Ctrl, Alt, Super…: pressed on their own, they shouldn't close the cheat sheet,
/// since they start the Ctrl+? that may be closing it.
fn is_modifier(key: gdk::Key) -> bool {
    use gdk::Key;
    matches!(
        key,
        Key::Shift_L | Key::Shift_R | Key::Control_L | Key::Control_R | Key::Alt_L | Key::Alt_R
            | Key::Super_L | Key::Super_R | Key::Meta_L | Key::Meta_R | Key::ISO_Level3_Shift
            | Key::Caps_Lock
    )
}

/// Opens a URI with the default app (browser, mail client, …).
fn launch(window: &gtk::ApplicationWindow, uri: &str) {
    gtk::UriLauncher::new(uri).launch(Some(window), None::<&gio::Cancellable>, |_| {});
}

/// Ctrl + scroll wheel zooms.
fn zoom_on_ctrl_scroll(scroller: &gtk::ScrolledWindow) {
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
    let pending = Cell::new(0.0);
    scroll.connect_scroll(move |controller, _, dy| {
        if !controller.current_event_state().contains(gdk::ModifierType::CONTROL_MASK) {
            return glib::Propagation::Proceed;
        }
        // Touchpads send many small deltas; zoom one step per whole notch.
        pending.set(pending.get() + dy);
        while pending.get() <= -1.0 {
            theme::zoom(1.0);
            pending.set(pending.get() + 1.0);
        }
        while pending.get() >= 1.0 {
            theme::zoom(-1.0);
            pending.set(pending.get() - 1.0);
        }
        glib::Propagation::Stop
    });
    scroller.add_controller(scroll);
}

/// Appended to the right-click menu of both views.
fn menu_model() -> gio::Menu {
    let section = |items: &[(&str, &str)]| {
        let menu = gio::Menu::new();
        for (label, action) in items {
            menu.append(Some(label), Some(action));
        }
        menu
    };
    let menu = gio::Menu::new();
    menu.append_section(None, &section(&[
        ("New Window", "app.new-window"),
        ("Open…", "win.open"),
        ("Save", "win.save"),
        ("Save As…", "win.save-as"),
    ]));
    menu.append_section(None, &section(&[("Find…", "win.find"), ("Replace…", "win.replace")]));
    menu.append_section(None, &section(&[("Keyboard Shortcuts", "win.shortcuts")]));
    menu.append_section(None, &section(&[
        ("Reading View", "win.reading"),
        ("Word Wrap", "win.wrap"),
        ("Line Numbers", "win.line-numbers"),
        ("Zoom In", "win.zoom-in"),
        ("Zoom Out", "win.zoom-out"),
        ("Reset Zoom", "win.zoom-reset"),
    ]));
    menu
}

/// Runs `f` once, right after the window paints its next frame.
pub fn after_next_frame(window: &gtk::ApplicationWindow, f: impl FnOnce() + 'static) {
    let Some(clock) = window.frame_clock() else { return f() };
    let pending = RefCell::new(Some(f));
    let handler = Rc::new(RefCell::new(None));
    let handler_in = handler.clone();
    let id = clock.connect_after_paint(move |clock| {
        if let Some(f) = pending.borrow_mut().take() {
            if let Some(id) = handler_in.borrow_mut().take() {
                clock.disconnect(id);
            }
            glib::idle_add_local_once(f);
        }
    });
    *handler.borrow_mut() = Some(id);
    window.queue_draw();
}

/// "/home/soheil/notes" → "~/notes"
fn tilde(path: &str) -> String {
    let home = glib::home_dir();
    match home.to_str().and_then(|home| path.strip_prefix(home)) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

fn spawn<T: 'static>(future: impl Future<Output = T> + 'static) {
    glib::spawn_future_local(future);
}

/// Shows a modal question; returns the index of the chosen button (0 = cancel).
async fn choose(parent: &gtk::ApplicationWindow, message: &str, detail: &str, buttons: &[&str]) -> Option<i32> {
    gtk::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .buttons(buttons)
        .cancel_button(0)
        .default_button(buttons.len() as i32 - 1)
        .modal(true)
        .build()
        .choose_future(Some(parent))
        .await
        .ok()
}

fn alert(parent: &gtk::ApplicationWindow, message: &str, detail: &str) {
    gtk::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .modal(true)
        .build()
        .show(Some(parent));
}

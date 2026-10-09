//! Spell checking (Settings → Writing), by libspelling. The library is loaded from disk only
//! when spell checking is turned on, so nevis neither pays for it at startup nor needs it
//! installed otherwise. Right-clicking a word offers corrections, adding it to the
//! dictionary, and the language, which is kept for next time.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::PathBuf;
use std::sync::OnceLock;

use gtk::glib::translate::{FromGlibPtrFull, FromGlibPtrNone, ToGlibPtr};
use gtk::{gio, glib, prelude::*};

unsafe extern "C" {
    fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}
const RTLD_NOW: c_int = 2;

type Init = unsafe extern "C" fn();
type Checker = unsafe extern "C" fn() -> *mut c_void;
type NewAdapter = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut glib::gobject_ffi::GObject;
type MenuOf = unsafe extern "C" fn(*mut glib::gobject_ffi::GObject) -> *mut gio::ffi::GMenuModel;
type Update = unsafe extern "C" fn(*mut glib::gobject_ffi::GObject);

/// The few libspelling functions nevis calls; the rest goes through GObject properties.
struct Library {
    checker: Checker,
    adapter: NewAdapter,
    menu: MenuOf,
    update: Update,
}

// The function pointers are plain code addresses, the same on every thread.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}

/// libspelling, loaded the first time it is asked for; `None` when it isn't installed.
fn library() -> Option<&'static Library> {
    static LIBRARY: OnceLock<Option<Library>> = OnceLock::new();
    LIBRARY
        .get_or_init(|| unsafe {
            let handle = dlopen(c"libspelling-1.so.2".as_ptr(), RTLD_NOW);
            if handle.is_null() {
                return None;
            }
            let find = |name: &CStr| {
                let address = dlsym(handle, name.as_ptr());
                (!address.is_null()).then_some(address)
            };
            let init = std::mem::transmute::<*mut c_void, Init>(find(c"spelling_init")?);
            init();
            Some(Library {
                checker: std::mem::transmute::<*mut c_void, Checker>(find(c"spelling_checker_get_default")?),
                adapter: std::mem::transmute::<*mut c_void, NewAdapter>(find(c"spelling_text_buffer_adapter_new")?),
                menu: std::mem::transmute::<*mut c_void, MenuOf>(find(c"spelling_text_buffer_adapter_get_menu_model")?),
                update: std::mem::transmute::<*mut c_void, Update>(find(c"spelling_text_buffer_adapter_update_corrections")?),
            })
        })
        .as_ref()
}

/// Spell checking for one buffer.
pub struct Spelling {
    /// libspelling's adapter: it underlines words and is the "spelling" action group.
    adapter: glib::Object,
    menu: gio::MenuModel,
}

impl Spelling {
    /// Checks `buffer`'s spelling, if libspelling is installed; starts turned off.
    pub fn new(buffer: &sourceview::Buffer) -> Option<Self> {
        let library = library()?;
        let (adapter, menu) = unsafe {
            let checker = (library.checker)();
            let buffer: *mut sourceview::ffi::GtkSourceBuffer = buffer.to_glib_none().0;
            let adapter = glib::Object::from_glib_full((library.adapter)(buffer.cast(), checker));
            let menu = gio::MenuModel::from_glib_none((library.menu)(adapter.as_ptr()));
            (adapter, menu)
        };
        if let Some(language) = language() {
            adapter.set_property("language", language);
        }
        // A language picked from the menu is kept for next time.
        adapter.connect_notify_local(Some("language"), |adapter, _| {
            if let Some(language) = adapter.property::<Option<String>>("language") {
                let _ = std::fs::write(language_file(), language);
            }
        });
        Some(Self { adapter, menu })
    }

    pub fn set_enabled(&self, on: bool) {
        self.adapter.set_property("enabled", on);
    }

    /// The actions its menu items run, to add to the view as "spelling".
    pub fn actions(&self) -> Option<gio::ActionGroup> {
        self.adapter.clone().dynamic_cast().ok()
    }

    /// Corrections for the word at the cursor, and the other spelling items.
    pub fn menu(&self) -> &gio::MenuModel {
        &self.menu
    }

    /// Fills the menu with corrections for the word at the cursor.
    pub fn update_corrections(&self) {
        if let Some(library) = library() {
            unsafe { (library.update)(self.adapter.as_ptr()) }
        }
    }
}

/// Whether libspelling can be loaded (for the Settings page).
pub fn available() -> bool {
    library().is_some()
}

fn language_file() -> PathBuf {
    glib::user_config_dir().join("nevis").join("spell-language")
}

/// The language picked last, if any.
fn language() -> Option<String> {
    let language = std::fs::read_to_string(language_file()).ok()?;
    let language = language.trim();
    (!language.is_empty()).then(|| language.to_owned())
}

//! Editor colours (a GtkSourceView style scheme) and the font/zoom stylesheet.

use std::cell::Cell;

use gtk::{gdk, glib};
use sourceview::StyleSchemeManager;

const SCHEME_ID: &str = "nevis-ayu-mirage";
const SCHEME_XML: &str = include_str!("../data/nevis-ayu-mirage.xml");

const DEFAULT_FONT_PT: f64 = 12.0;
const MIN_FONT_PT: f64 = 7.0;
const MAX_FONT_PT: f64 = 48.0;

thread_local! {
    static FONT_PT: Cell<f64> = const { Cell::new(DEFAULT_FONT_PT) };
    static FONT_CSS: gtk::CssProvider = gtk::CssProvider::new();
}

/// Registers the bundled style scheme and the app stylesheet. Call once at startup.
pub fn init() {
    // GtkSourceView only loads schemes from files, so keep a copy in the user data dir.
    let dir = glib::user_data_dir().join("nevis").join("styles");
    let path = dir.join(format!("{SCHEME_ID}.xml"));
    if std::fs::read_to_string(&path).ok().as_deref() != Some(SCHEME_XML) {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&path, SCHEME_XML);
    }
    if let Some(dir) = dir.to_str() {
        StyleSchemeManager::default().append_search_path(dir);
    }

    let Some(display) = gdk::Display::default() else { return };
    let chrome = gtk::CssProvider::new();
    chrome.load_from_string(
        ".nevis-find { padding: 6px 10px; border-bottom: 1px solid alpha(currentColor, 0.12); }
         .nevis-table { border: 1px solid #3D4A5C; border-radius: 6px; background: #1A1F29; }
         .nevis-help-backdrop { background: alpha(black, 0.45); }
         .nevis-help { background: #1A1F29; color: #CCCAC2; border: 1px solid #3D4A5C; border-radius: 12px;
                       padding: 20px 28px; box-shadow: 0 8px 32px alpha(black, 0.5); }
         .nevis-help-title { font-size: 1.3em; font-weight: bold; }
         .nevis-help-section { color: #FFCC66; font-weight: bold; }
         .nevis-help-keys { font-family: \"JetBrains Mono\", monospace; color: #95E6CB; }",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &chrome,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    FONT_CSS.with(|css| {
        gtk::style_context_add_provider_for_display(
            &display,
            css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
    apply_font();
}

pub fn scheme() -> Option<sourceview::StyleScheme> {
    StyleSchemeManager::default().scheme(SCHEME_ID)
}

/// Changes the editor font size by `steps` points (shared by all windows); 0 resets it.
pub fn zoom(steps: f64) {
    let size = if steps == 0.0 {
        DEFAULT_FONT_PT
    } else {
        (FONT_PT.get() + steps).clamp(MIN_FONT_PT, MAX_FONT_PT)
    };
    FONT_PT.set(size);
    apply_font();
}

fn apply_font() {
    // Vazirmatn covers Persian, which the Latin fonts lack.
    let size = FONT_PT.get();
    let css = format!(
        "textview.nevis-editor {{ font-family: \"JetBrains Mono\", \"Vazirmatn\", monospace; font-size: {size}pt; }}
         textview.nevis-read {{ font-family: \"Inter Variable\", \"Inter\", \"Vazirmatn\", sans-serif; font-size: {size}pt; }}"
    );
    FONT_CSS.with(|provider| provider.load_from_string(&css));
}

/// Ayu Mirage colours for the read view (the editor gets the same ones from the style scheme).
pub mod palette {
    /// Vazirmatn after the code font: without it Persian falls back to a monospace font
    /// that draws the letters unjoined.
    pub const MONO: &str = "JetBrains Mono, Vazirmatn";
    pub const HEADING: &str = "#FFCC66";
    pub const ACCENT: &str = "#FFCC66";
    pub const LINK: &str = "#73D0FF";
    pub const CODE: &str = "#95E6CB";
    pub const CODE_BG: &str = "#191E2A";
    pub const QUOTE_BG: &str = "#232834";
    pub const DIM_FG: &str = "#B8B6AE";
    pub const DIM: &str = "#707A8C";
    pub const RULE: &str = "#3D4A5C";
}

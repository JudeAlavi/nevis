//! The colours of the GTK theme in use, as named in the user's GTK 4 stylesheet:
//! `@define-color` and `--custom-property` definitions (what matugen, noctalia or pywal write
//! there), following its `@import`s.

use std::collections::HashMap;

use crate::color::Rgb;

/// Colour definitions by name (dashes read as underscores), as written.
#[derive(Debug, Default)]
pub struct Named(HashMap<String, String>);

impl Named {
    /// Reads `css`; `import` gives the text of a stylesheet that an `@import` names.
    pub fn read(css: &str, import: &mut dyn FnMut(&str) -> Option<String>) -> Named {
        let mut named = Named::default();
        named.take_in(css, import, &mut Vec::new());
        named
    }

    /// Adds the definitions in `css` in order, an import's where it stands; `seen` holds
    /// the stylesheets already imported, so that none is read twice.
    fn take_in(&mut self, css: &str, import: &mut dyn FnMut(&str) -> Option<String>, seen: &mut Vec<String>) {
        for statement in without_comments(css).split(';') {
            // The declaration after any selector and braces before it.
            let statement = statement.rsplit(['{', '}']).next().unwrap_or_default().trim();
            if let Some(rest) = statement.strip_prefix("@define-color") {
                if let Some((name, value)) = rest.trim_start().split_once(char::is_whitespace) {
                    self.0.insert(name.to_owned(), value.trim().to_owned());
                }
            } else if let Some(rest) = statement.strip_prefix("--")
                && let Some((name, value)) = rest.split_once(':')
            {
                self.0.insert(name.trim().replace('-', "_"), value.trim().to_owned());
            } else if let Some(rest) = statement.strip_prefix("@import") {
                let url = import_url(rest);
                if !seen.iter().any(|done| *done == url) && seen.len() < 32 {
                    seen.push(url.to_owned());
                    if let Some(text) = import(url) {
                        self.take_in(&text, import, seen);
                    }
                }
            }
        }
    }

    /// The colour `name` stands for, references and colour functions worked out.
    pub fn color(&self, name: &str) -> Option<Rgb> {
        self.named(name, 0)
    }

    /// `depth` counts references followed: a definition that leads back to itself ends.
    fn named(&self, name: &str, depth: usize) -> Option<Rgb> {
        if depth > 16 {
            return None;
        }
        self.evaluate(self.0.get(name)?, depth + 1)
    }

    /// A colour value: "#hex", "rgb(…)", "@name", "var(--name)", a few colour names, or one
    /// of GTK's functions: alpha(), mix(), shade(), lighter(), darker().
    fn evaluate(&self, value: &str, depth: usize) -> Option<Rgb> {
        let value = value.trim();
        if value.starts_with('#') {
            return Rgb::hex(value);
        }
        if let Some(name) = value.strip_prefix('@') {
            return self.named(name, depth);
        }
        match value {
            "transparent" => return Some(Rgb { a: 0.0, ..Rgb::BLACK }),
            "white" => return Some(Rgb::WHITE),
            "black" => return Some(Rgb::BLACK),
            _ => {}
        }
        let (function, arguments) = value.strip_suffix(')')?.split_once('(')?;
        let parts = split_arguments(arguments);
        let color = |i: usize| self.evaluate(parts.get(i)?, depth);
        let number = |i: usize| parts.get(i)?.trim().parse::<f64>().ok();
        match function.trim() {
            "rgb" | "rgba" => rgb(arguments),
            "var" => self.named(&parts.first()?.trim().strip_prefix("--")?.replace('-', "_"), depth),
            "alpha" => {
                let base = color(0)?;
                Some(Rgb { a: base.a * number(1)?, ..base })
            }
            "mix" => Some(color(0)?.mix(color(1)?, number(2)?)),
            "shade" => Some(shade(color(0)?, number(1)?)),
            "lighter" => Some(shade(color(0)?, 1.3)),
            "darker" => Some(shade(color(0)?, 0.7)),
            _ => None,
        }
    }
}

/// "a, f(b, c), d" split at its outer commas.
fn split_arguments(text: &str) -> Vec<&str> {
    let (mut parts, mut start, mut depth) = (Vec::new(), 0, 0);
    for (at, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// GTK's shade(): lightness and saturation scaled by `factor`.
fn shade(color: Rgb, factor: f64) -> Rgb {
    let (max, min) = (color.r.max(color.g).max(color.b), color.r.min(color.g).min(color.b));
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    let saturation = if delta == 0.0 { 0.0 } else { delta / (1.0 - (2.0 * lightness - 1.0).abs()) };
    let hue = if delta == 0.0 {
        0.0
    } else if max == color.r {
        60.0 * ((color.g - color.b) / delta).rem_euclid(6.0)
    } else if max == color.g {
        60.0 * ((color.b - color.r) / delta + 2.0)
    } else {
        60.0 * ((color.r - color.g) / delta + 4.0)
    };
    let (lightness, saturation) = ((lightness * factor).clamp(0.0, 1.0), (saturation * factor).clamp(0.0, 1.0));
    // Back from hue, saturation and lightness.
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = chroma * (1.0 - ((hue / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    Rgb { r: r + m, g: g + m, b: b + m, a: color.a }
}

/// The stylesheet an @import names: `'a.css'`, `"a.css"`, `url("a.css")` or `url(a.css)`.
fn import_url(rest: &str) -> &str {
    let rest = rest.trim();
    let inner = match rest.strip_prefix("url(") {
        Some(inside) => inside.split(')').next().unwrap_or_default(),
        None => rest.split_whitespace().next().unwrap_or_default(),
    };
    inner.trim().trim_matches(['"', '\''])
}

/// `css` with its /* comments */ taken out.
fn without_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..].find("*/").map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    out.push_str(rest);
    out
}

/// "225, 226, 232", "29 32 36 / 50%", "100%, 0, 0, 0.5": red, green, blue and maybe alpha.
fn rgb(arguments: &str) -> Option<Rgb> {
    let parts: Vec<&str> = arguments.split([',', '/', ' ']).map(str::trim).filter(|p| !p.is_empty()).collect();
    let number = |text: &str, whole: f64| match text.strip_suffix('%') {
        Some(percent) => percent.trim().parse::<f64>().ok().map(|p| p / 100.0),
        None => text.parse::<f64>().ok().map(|n| n / whole),
    };
    let (r, g, b) = (number(parts.first()?, 255.0)?, number(parts.get(1)?, 255.0)?, number(parts.get(2)?, 255.0)?);
    let a = match parts.get(3) {
        Some(alpha) => number(alpha, 1.0)?,
        None => 1.0,
    };
    Some(Rgb { r, g, b, a })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(css: &str) -> Named {
        Named::read(css, &mut |_| None)
    }

    fn hex(named: &Named, name: &str) -> Option<String> {
        named.color(name).map(Rgb::to_hex)
    }

    #[test]
    fn defined_colours_are_read() {
        let colours = named(
            "@define-color window_bg_color #101418;
             @define-color window_fg_color rgb(225, 226, 232);
             @define-color card_bg_color rgba(29, 32, 36, 0.5);",
        );
        assert_eq!(hex(&colours, "window_bg_color").as_deref(), Some("#101418"));
        assert_eq!(hex(&colours, "window_fg_color").as_deref(), Some("#e1e2e8"));
        assert_eq!(colours.color("card_bg_color").map(|c| c.a), Some(0.5));
        assert_eq!(colours.color("accent_color"), None);
    }

    #[test]
    fn custom_properties_count_and_comments_do_not() {
        let colours = named(
            "/* @define-color window_bg_color #ff0000; */
             :root {
                 --accent-bg-color: #9fcafd;
                 --warning-color: #d6bee5; /* a comment after */
             }
             window { color: red; }",
        );
        assert_eq!(hex(&colours, "accent_bg_color").as_deref(), Some("#9fcafd"));
        assert_eq!(hex(&colours, "warning_color").as_deref(), Some("#d6bee5"));
        assert_eq!(colours.color("window_bg_color"), None);
    }

    #[test]
    fn references_and_colour_functions_are_worked_out() {
        let colours = named(
            "@define-color accent_bg_color #9fcafd;
             @define-color accent_color @accent_bg_color;
             @define-color hover alpha(@accent_bg_color, 0.08);
             @define-color quarter mix(#000000, white, 0.25);
             @define-color brighter shade(#808080, 1.3);
             @define-color dimmer darker(#808080);
             :root { --view-fg-color: var(--accent-bg-color); }
             @define-color loop @loop;",
        );
        assert_eq!(hex(&colours, "accent_color").as_deref(), Some("#9fcafd"));
        assert_eq!(colours.color("hover").map(|c| (c.to_hex(), c.a)), Some(("#9fcafd".to_owned(), 0.08)));
        assert_eq!(hex(&colours, "quarter").as_deref(), Some("#404040"));
        // shade() scales lightness: 0.502 × 1.3 = 0.653 → 166; darker() is shade(0.7) → 90.
        assert_eq!(hex(&colours, "brighter").as_deref(), Some("#a6a6a6"));
        assert_eq!(hex(&colours, "dimmer").as_deref(), Some("#5a5a5a"));
        assert_eq!(hex(&colours, "view_fg_color").as_deref(), Some("#9fcafd"));
        assert_eq!(colours.color("loop"), None);
    }

    #[test]
    fn imports_are_followed_where_they_stand() {
        let files = HashMap::from([
            ("colors.css", "@define-color window_bg_color #111111; @define-color accent_bg_color #222222;"),
            ("noctalia.css", "@define-color accent_bg_color #9fcafd; @import 'noctalia.css';"),
        ]);
        let colours = Named::read(
            "@define-color window_bg_color #000000;
             @import 'colors.css';
             @import url(\"noctalia.css\");
             @define-color accent_color @accent_bg_color;",
            &mut |url| files.get(url).map(|text| text.to_string()),
        );
        // Each import overrides what came before it; noctalia.css importing itself is skipped.
        assert_eq!(hex(&colours, "window_bg_color").as_deref(), Some("#111111"));
        assert_eq!(hex(&colours, "accent_color").as_deref(), Some("#9fcafd"));
    }
}

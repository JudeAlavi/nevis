//! Colours and the little maths themes need: mixing, contrast, keeping text readable.

/// An sRGB colour; components from 0 to 1, alpha included.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Rgb = Rgb { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

    /// "#rgb", "#rgba", "#rrggbb" or "#rrggbbaa".
    pub fn hex(text: &str) -> Option<Rgb> {
        let digits = text.strip_prefix('#')?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        // Each component as 0–255: one digit stands for two ("f" is "ff").
        let short = |i: usize| u8::from_str_radix(&digits[i..=i], 16).ok().map(|v| v * 17);
        let long = |i: usize| u8::from_str_radix(&digits[2 * i..2 * i + 2], 16).ok();
        let parts: Vec<u8> = match digits.len() {
            3 | 4 => (0..digits.len()).map(short).collect::<Option<_>>()?,
            6 | 8 => (0..digits.len() / 2).map(long).collect::<Option<_>>()?,
            _ => return None,
        };
        let unit = |v: u8| f64::from(v) / 255.0;
        Some(Rgb { r: unit(parts[0]), g: unit(parts[1]), b: unit(parts[2]), a: parts.get(3).map_or(1.0, |&a| unit(a)) })
    }

    /// `self` moved towards `other` by `amount` (0 keeps self, 1 gives other).
    pub fn mix(self, other: Rgb, amount: f64) -> Rgb {
        let blend = |a: f64, b: f64| a + (b - a) * amount;
        Rgb { r: blend(self.r, other.r), g: blend(self.g, other.g), b: blend(self.b, other.b), a: blend(self.a, other.a) }
    }

    /// How `self` shows over an opaque `below`, given its alpha.
    pub fn over(self, below: Rgb) -> Rgb {
        Rgb { a: 1.0, ..below.mix(Rgb { a: 1.0, ..self }, self.a) }
    }

    /// Relative luminance, as WCAG defines it.
    pub fn luminance(self) -> f64 {
        let linear = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// WCAG contrast ratio with `other`: 1 (none) to 21 (black on white).
    pub fn contrast(self, other: Rgb) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Lightness (0–1), chroma and hue (degrees) in OKLCH, where equal steps look equal.
    pub fn oklch(self) -> (f64, f64, f64) {
        let linear = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let (r, g, b) = (linear(self.r), linear(self.g), linear(self.b));
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        let lightness = 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s;
        let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
        let bb = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
        (lightness, a.hypot(bb), bb.atan2(a).to_degrees().rem_euclid(360.0))
    }

    /// The colour at OKLCH `lightness`, `chroma` and `hue`, brought into the sRGB range.
    pub fn from_oklch(lightness: f64, chroma: f64, hue: f64) -> Rgb {
        let (a, b) = (chroma * hue.to_radians().cos(), chroma * hue.to_radians().sin());
        let l = (lightness + 0.3963377774 * a + 0.2158037573 * b).powi(3);
        let m = (lightness - 0.1055613458 * a - 0.0638541728 * b).powi(3);
        let s = (lightness - 0.0894841775 * a - 1.2914855480 * b).powi(3);
        let gamma = |c: f64| {
            let c = c.clamp(0.0, 1.0);
            if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
        };
        Rgb {
            r: gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
            g: gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
            b: gamma(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
            a: 1.0,
        }
    }

    /// `self`, lightened (on dark backgrounds) or darkened (on light ones) just enough to
    /// reach `ratio` of contrast with `background`, keeping its hue.
    pub fn readable_on(self, background: Rgb, ratio: f64) -> Rgb {
        if self.contrast(background) >= ratio {
            return self;
        }
        let (lightness, chroma, hue) = self.oklch();
        // Towards white or black, whichever can stand out more from the background.
        let target = if Rgb::WHITE.contrast(background) >= Rgb::BLACK.contrast(background) { 1.0 } else { 0.0 };
        let (mut near, mut far) = (lightness, target);
        for _ in 0..24 {
            let middle = (near + far) / 2.0;
            if Rgb::from_oklch(middle, chroma, hue).contrast(background) >= ratio {
                far = middle;
            } else {
                near = middle;
            }
        }
        Rgb { a: self.a, ..Rgb::from_oklch(far, chroma, hue) }
    }

    /// "#rrggbb", alpha left out.
    pub fn to_hex(self) -> String {
        let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", byte(self.r), byte(self.g), byte(self.b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_read_and_write_as_hex() {
        assert_eq!(Rgb::hex("#1F2430").map(Rgb::to_hex).as_deref(), Some("#1f2430"));
        assert_eq!(Rgb::hex("#fff"), Some(Rgb::WHITE));
        assert_eq!(Rgb::hex("#00000080").map(|c| (c.a * 255.0).round()), Some(128.0));
        assert_eq!(Rgb::hex("#f008").map(|c| (c.to_hex(), (c.a * 255.0).round())), Some(("#ff0000".to_owned(), 136.0)));
        for bad in ["", "#", "#12345", "1F2430", "#gg0000"] {
            assert_eq!(Rgb::hex(bad), None, "{bad}");
        }
    }

    fn hex(text: &str) -> Rgb {
        Rgb::hex(text).unwrap()
    }

    #[test]
    fn mixing_and_contrast() {
        assert_eq!(Rgb::BLACK.mix(Rgb::WHITE, 0.5).to_hex(), "#808080");
        assert_eq!(hex("#102030").mix(hex("#ffffff"), 0.0).to_hex(), "#102030");
        assert_eq!(hex("#102030").mix(hex("#ffffff"), 1.0).to_hex(), "#ffffff");
        // WCAG contrast ratios.
        assert_eq!((Rgb::BLACK.contrast(Rgb::WHITE) * 100.0).round(), 2100.0);
        assert!((4.47..4.49).contains(&hex("#777777").contrast(Rgb::WHITE)));
        assert_eq!(hex("#1f2430").contrast(hex("#1f2430")), 1.0);
    }

    #[test]
    fn faint_colours_are_made_readable_keeping_their_hue() {
        let night = hex("#101418");
        let faint = hex("#30404f");
        let fixed = faint.readable_on(night, 4.5);
        assert!(fixed.contrast(night) >= 4.5);
        assert!((faint.oklch().2 - fixed.oklch().2).abs() < 5.0, "hue kept");
        // On a light background, darker instead.
        let paper = hex("#fbf1c7");
        let pale = hex("#e0d070");
        let fixed = pale.readable_on(paper, 3.0);
        assert!(fixed.contrast(paper) >= 3.0 && fixed.luminance() < pale.luminance());
        // Readable already: unchanged.
        assert_eq!(hex("#ffcc66").readable_on(night, 4.5), hex("#ffcc66"));
    }
}

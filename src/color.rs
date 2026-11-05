//! Perceptual colour maths: sRGB -> CIELAB and colour distance.
//!
//! Distances are CIE76 ΔE (Euclidean in Lab). Rough guide: ~2.3 is a
//! just-noticeable difference, ~10 is "same colour, different shade",
//! 25+ is clearly a different colour.

pub type Rgb = [u8; 3];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

impl Lab {
    pub fn from_rgb([r, g, b]: Rgb) -> Self {
        fn linear(c: u8) -> f32 {
            let c = c as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        let (r, g, b) = (linear(r), linear(g), linear(b));

        // Linear sRGB -> XYZ (D65), normalised by the D65 white point.
        let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;

        fn f(t: f32) -> f32 {
            const D: f32 = 6.0 / 29.0;
            if t > D * D * D {
                t.cbrt()
            } else {
                t / (3.0 * D * D) + 4.0 / 29.0
            }
        }
        let (fx, fy, fz) = (f(x), f(y), f(z));
        Lab {
            l: 116.0 * fy - 16.0,
            a: 500.0 * (fx - fy),
            b: 200.0 * (fy - fz),
        }
    }

    pub fn delta_e(self, other: Lab) -> f32 {
        self.delta_e_squared(other).sqrt()
    }

    /// ΔE², for comparisons where the square root isn't needed.
    pub fn delta_e_squared(self, other: Lab) -> f32 {
        let (dl, da, db) = (self.l - other.l, self.a - other.a, self.b - other.b);
        dl * dl + da * da + db * db
    }
}

pub fn to_hex([r, g, b]: Rgb) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Parses `#RRGGBB`, `RRGGBB`, or the short `#RGB` form.
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let s = s.trim().trim_start_matches('#');
    let digits: Vec<u8> = s
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<_>>()?;
    match digits[..] {
        [r, g, b] => Some([r * 17, g * 17, b * 17]),
        [r1, r2, g1, g2, b1, b2] => Some([r1 << 4 | r2, g1 << 4 | g2, b1 << 4 | b2]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lab_reference_values() {
        let white = Lab::from_rgb([255, 255, 255]);
        assert!((white.l - 100.0).abs() < 0.1 && white.a.abs() < 0.1 && white.b.abs() < 0.1);
        let red = Lab::from_rgb([255, 0, 0]);
        assert!(
            (red.l - 53.24).abs() < 0.1
                && (red.a - 80.09).abs() < 0.2
                && (red.b - 67.20).abs() < 0.2
        );
    }

    #[test]
    fn near_colours_are_closer_than_far_ones() {
        let yellow = Lab::from_rgb([255, 220, 0]);
        assert!(yellow.delta_e(Lab::from_rgb([250, 210, 10])) < 10.0);
        assert!(yellow.delta_e(Lab::from_rgb([40, 80, 200])) > 50.0);
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(parse_hex("#ffcc00"), Some([255, 204, 0]));
        assert_eq!(parse_hex("FC0"), Some([255, 204, 0]));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("zzzzzz"), None);
        assert_eq!(to_hex([255, 204, 0]), "#FFCC00");
    }
}

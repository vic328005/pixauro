//! sRGB and CIE L*a*b* conversion, both relative to the sRGB D65 white point.
//!
//! The D65 chromaticity is `(x, y) = (0.3127, 0.3290)` and XYZ is normalized
//! to `Y = 1`. The sRGB transfer functions and high-precision XYZ matrices
//! follow <https://www.w3.org/TR/css-color-4/#color-conversion-code>.
//! Unlike CSS `lab()`, this module does **not** adapt XYZ to D50. Using the
//! rounded D65 tristimulus values `(0.95047, 1, 1.08883)` instead will produce
//! slightly different Lab coordinates.

use std::sync::OnceLock;

/// CIE L*a*b* coordinates `[L*, a*, b*]`, relative to the sRGB D65 white point.
///
/// `L*` is normally in `0..=100`; `a*` and `b*` are signed color coordinates.
pub type Lab = [f64; 3];

const WHITE: [f64; 3] = [3127.0 / 3290.0, 1.0, 3583.0 / 3290.0];
const EPSILON: f64 = 216.0 / 24389.0;
const KAPPA: f64 = 24389.0 / 27.0;
const DELTA: f64 = 6.0 / 29.0;

const RGB_TO_XYZ: [[f64; 3]; 3] = [
    [506752.0 / 1228815.0, 87881.0 / 245763.0, 12673.0 / 70218.0],
    [87098.0 / 409605.0, 175762.0 / 245763.0, 12673.0 / 175545.0],
    [7918.0 / 409605.0, 87881.0 / 737289.0, 1001167.0 / 1053270.0],
];

const XYZ_TO_RGB: [[f64; 3]; 3] = [
    [12831.0 / 3959.0, -329.0 / 214.0, -1974.0 / 3959.0],
    [
        -851781.0 / 878810.0,
        1648619.0 / 878810.0,
        36519.0 / 878810.0,
    ],
    [705.0 / 12673.0, -2585.0 / 12673.0, 705.0 / 667.0],
];

// Input channels have only 256 possible values. Initializing this table once
// avoids three powf calls per source pixel and is safe across Rayon workers.
static LINEAR_SRGB: OnceLock<[f64; 256]> = OnceLock::new();

#[inline]
fn decode_srgb(channel: f64) -> f64 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
fn lab_curve(value: f64) -> f64 {
    if value > EPSILON {
        value.cbrt()
    } else {
        (KAPPA * value + 16.0) / 116.0
    }
}

#[inline]
fn inverse_lab_curve(value: f64) -> f64 {
    if value > DELTA {
        value * value * value
    } else {
        (116.0 * value - 16.0) / KAPPA
    }
}

#[inline]
fn multiply(matrix: &[[f64; 3]; 3], vector: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| row[0] * vector[0] + row[1] * vector[1] + row[2] * vector[2])
}

/// Converts an opaque, gamma-encoded 8-bit sRGB color to D65 CIE L*a*b*.
///
/// The piecewise sRGB transfer function is applied before conversion to XYZ.
/// All arithmetic and the shared 256-entry linearization table use `f64`.
#[inline]
pub fn srgb_to_lab(rgb: [u8; 3]) -> Lab {
    let table = LINEAR_SRGB.get_or_init(|| std::array::from_fn(|i| decode_srgb(i as f64 / 255.0)));
    let linear = rgb.map(|channel| table[usize::from(channel)]);
    let xyz = multiply(&RGB_TO_XYZ, linear);
    let f = std::array::from_fn::<_, 3, _>(|i| lab_curve(xyz[i] / WHITE[i]));
    [
        116.0 * f[1] - 16.0,
        500.0 * (f[0] - f[1]),
        200.0 * (f[1] - f[2]),
    ]
}

#[inline]
fn encode_channel(linear: f64) -> u8 {
    // Clipping before the transfer function avoids powf of negative values.
    // NaN is handled explicitly, including overflow of unreasonable Lab input.
    let linear = if linear.is_nan() {
        0.0
    } else {
        linear.clamp(0.0, 1.0)
    };
    let encoded = if linear <= 0.0031308 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

/// Converts D65 CIE L*a*b* to an opaque, gamma-encoded 8-bit sRGB color.
///
/// Out-of-gamut linear RGB channels are individually clipped to `0..=1`, then
/// gamma encoded and rounded to the nearest byte (`f64::round`, ties upward).
/// This is channel clipping, not perceptual gamut mapping. Lab coordinates
/// themselves are not clamped. Any non-finite input coordinate returns black.
#[inline]
pub fn lab_to_srgb(lab: Lab) -> [u8; 3] {
    if lab.iter().any(|component| !component.is_finite()) {
        return [0; 3];
    }
    let fy = (lab[0] + 16.0) / 116.0;
    let f = [fy + lab[1] / 500.0, fy, fy - lab[2] / 200.0];
    let xyz = std::array::from_fn(|i| inverse_lab_curve(f[i]) * WHITE[i]);
    multiply(&XYZ_TO_RGB, xyz).map(encode_channel)
}

#[inline]
pub(crate) fn squared_distance(a: Lab, b: Lab) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// Euclidean CIE76 color distance, not its square.
#[inline]
pub(crate) fn distance(a: Lab, b: Lab) -> f64 {
    squared_distance(a, b).sqrt()
}

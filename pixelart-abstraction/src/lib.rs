//! Coupled image segmentation and palette optimization, following Gerstner et al.,
//! *Pixelated Image Abstraction with Integrated User Constraints* (§4–5).
//!
//! The default follows the paper's **unsquared** Euclidean distance in Eq. (2).
//! See `docs/algorithm.md` for its theoretical inconsistency with the stated
//! critical temperature, and for parameters the paper leaves unspecified.
//!
//! ```text
//! use pixelart_abstraction::{pixelate, Options, RgbImage};
//! let input = RgbImage::new(16, 16, vec![[180, 80, 40]; 256])?;
//! let result = pixelate(&input, &Options::new(4, 4, 8))?;
//! assert_eq!(result.indices.len(), 16);
//! assert!(result.palette.len() <= 8);
//! # Ok::<(), pixelart_abstraction::Error>(())
//! ```

pub mod color;
mod math;
mod palette;
mod session;
mod spatial;

pub use session::Pixelator;
use std::fmt;

/// Errors detected before modifying a session or allocating its working buffers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Image dimensions are zero, overflow, or do not match the supplied data.
    InvalidImage(String),
    /// A numeric option is invalid or the requested output is larger than the input.
    InvalidOptions(String),
    /// Importance values must be finite, in [0, 1], match the input, and have positive sum.
    InvalidImportance(String),
    /// A constraint references a missing pixel/color, or allows no colors.
    InvalidConstraint(String),
    /// Pixel and palette edits require an initial completed call to `run`.
    NotYetRun,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidImage(s) => write!(f, "invalid image: {s}"),
            Self::InvalidOptions(s) => write!(f, "invalid options: {s}"),
            Self::InvalidImportance(s) => write!(f, "invalid importance map: {s}"),
            Self::InvalidConstraint(s) => write!(f, "invalid constraint: {s}"),
            Self::NotYetRun => write!(
                f,
                "run the initial abstraction before editing its palette or pixels"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// A checked, row-major, opaque sRGB image. No image I/O dependency is required.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbImage {
    width: usize,
    height: usize,
    pixels: Vec<[u8; 3]>,
    alpha: Option<Vec<u8>>,
}

impl RgbImage {
    /// Construct an image; dimensions must be positive and match the data length.
    pub fn new(width: usize, height: usize, pixels: Vec<[u8; 3]>) -> Result<Self, Error> {
        if width == 0 || height == 0 || width.checked_mul(height) != Some(pixels.len()) {
            return Err(Error::InvalidImage(
                "positive width × height must equal the pixel count".into(),
            ));
        }
        Ok(Self {
            width,
            height,
            pixels,
            alpha: None,
        })
    }

    /// Copy interleaved RGB bytes into an image, rejecting incomplete pixels.
    pub fn from_rgb_bytes(width: usize, height: usize, bytes: &[u8]) -> Result<Self, Error> {
        let expected = width.checked_mul(height).and_then(|n| n.checked_mul(3));
        if width == 0 || height == 0 || expected != Some(bytes.len()) {
            return Err(Error::InvalidImage(
                "RGB byte length must be 3 × width × height".into(),
            ));
        }
        Self::new(width, height, bytes.as_chunks::<3>().0.to_vec())
    }

    /// Straight-alpha sRGB; hidden RGB never contributes to the solver.
    pub fn from_rgba(width: usize, height: usize, rgba: &[[u8; 4]]) -> Result<Self, Error> {
        let mut image = Self::new(
            width,
            height,
            rgba.iter().map(|p| [p[0], p[1], p[2]]).collect(),
        )?;
        if rgba.iter().any(|p| p[3] != 255) {
            image.alpha = Some(rgba.iter().map(|p| p[3]).collect());
        }
        Ok(image)
    }

    /// Optional per-input opacity, absent for fully opaque images.
    pub fn alpha(&self) -> Option<&[u8]> {
        self.alpha.as_deref()
    }

    /// Width in pixels.
    pub fn width(&self) -> usize {
        self.width
    }
    /// Height in pixels.
    pub fn height(&self) -> usize {
        self.height
    }
    /// Row-major sRGB triples.
    pub fn pixels(&self) -> &[[u8; 3]] {
        &self.pixels
    }
    /// Copy to interleaved RGB bytes.
    pub fn to_rgb_bytes(&self) -> Vec<u8> {
        self.pixels.iter().flatten().copied().collect()
    }
}

/// Distance used **only** by the annealing probability model (Eq. 2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnnealingDistance {
    /// The literal, unsquared norm printed in the supplied journal paper.
    #[default]
    Euclidean,
    /// Explicit squared-distortion DA variant. This changes the algorithm.
    SquaredEuclidean,
}

/// Bilateral smoothing on the low-resolution grid of superpixel mean colors.
#[derive(Clone, Debug)]
pub struct BilateralOptions {
    /// Square kernel radius in output pixels; zero disables filtering.
    pub radius: usize,
    /// Spatial Gaussian standard deviation, in output pixels.
    pub spatial_sigma: f64,
    /// Color Gaussian standard deviation, in CIELAB distance units.
    pub color_sigma: f64,
}

impl Default for BilateralOptions {
    fn default() -> Self {
        Self {
            radius: 2,
            spatial_sigma: 1.0,
            color_sigma: 3.0,
        }
    }
}

/// Algorithm configuration. Paper-specified defaults and implementation choices
/// are distinguished in `docs/algorithm.md`.
#[derive(Clone, Debug)]
pub struct Options {
    /// Output width, at most the input width.
    pub width: usize,
    /// Output height, at most the input height.
    pub height: usize,
    /// Maximum number of colors (1..=256). Fewer may be needed.
    pub colors: usize,
    /// Position weight `m` in Eq. (1); default 45.
    pub compactness: f64,
    /// Four-neighbor center smoothing fraction; default 0.4.
    pub position_smoothing: f64,
    /// Cooling multiplier; default 0.7.
    pub cooling_rate: f64,
    /// Final temperature, at which convergence is also required; default 1.
    pub final_temperature: f64,
    /// Sum of component center displacements below which a palette has converged.
    pub palette_tolerance: f64,
    /// Minimum pair distance for a logical color to split.
    pub split_threshold: f64,
    /// Symmetric displacement of child components along their principal axis.
    pub perturbation: f64,
    /// Multiply final Lab a,b by this factor (not applied to locked RGB colors).
    pub saturation: f64,
    /// Maximum coupled iterations per call to `run`; exhaustion is reported explicitly.
    pub max_iterations: usize,
    /// Use Rayon when the `parallel` feature is enabled; serial otherwise.
    pub parallel: bool,
    /// Probability distance; default follows the paper as printed.
    pub annealing_distance: AnnealingDistance,
    /// Bilateral filter settings; its exact kernel is not specified by the paper.
    pub bilateral: BilateralOptions,
}

impl Options {
    /// Paper-oriented defaults at the requested output size and palette limit.
    pub fn new(width: usize, height: usize, colors: usize) -> Self {
        Self {
            width,
            height,
            colors,
            compactness: 45.0,
            position_smoothing: 0.4,
            cooling_rate: 0.7,
            final_temperature: 1.0,
            palette_tolerance: 1.0,
            split_threshold: 1.0,
            perturbation: 0.5,
            saturation: 1.1,
            max_iterations: 500,
            parallel: true,
            annealing_distance: AnnealingDistance::Euclidean,
            bilateral: BilateralOptions::default(),
        }
    }

    pub(crate) fn validate(&self, input: &RgbImage) -> Result<(), Error> {
        if self.width == 0
            || self.height == 0
            || self.width > input.width
            || self.height > input.height
        {
            return Err(Error::InvalidOptions(
                "output dimensions must be positive and no larger than input dimensions".into(),
            ));
        }
        if !(1..=256).contains(&self.colors) || self.max_iterations == 0 {
            return Err(Error::InvalidOptions(
                "colors must be in 1..=256 and max_iterations must be positive".into(),
            ));
        }
        for (name, value) in [
            ("compactness", self.compactness),
            ("final_temperature", self.final_temperature),
            ("palette_tolerance", self.palette_tolerance),
            ("split_threshold", self.split_threshold),
            ("perturbation", self.perturbation),
            ("spatial_sigma", self.bilateral.spatial_sigma),
            ("color_sigma", self.bilateral.color_sigma),
        ] {
            if !value.is_finite() || !(1.0e-12..=1.0e6).contains(&value) {
                return Err(Error::InvalidOptions(format!(
                    "{name} must be finite and in [1e-12, 1e6]"
                )));
            }
        }
        if !self.position_smoothing.is_finite()
            || !(0.0..=1.0).contains(&self.position_smoothing)
            || !self.cooling_rate.is_finite()
            || !(0.0..1.0).contains(&self.cooling_rate)
            || self.cooling_rate == 0.0
            || !self.saturation.is_finite()
            || !(0.0..=10.0).contains(&self.saturation)
        {
            return Err(Error::InvalidOptions(
                "smoothing must be in [0,1], cooling in (0,1), saturation in [0,10]".into(),
            ));
        }
        if self.bilateral.radius > 16 {
            return Err(Error::InvalidOptions(
                "bilateral radius must be at most 16".into(),
            ));
        }
        Ok(())
    }
}

/// Why the most recent run stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The palette converged at the final temperature.
    Converged,
    /// The iteration budget was reached; inspect or continue the same session.
    IterationLimit,
    /// The caller stopped a progress callback with `ControlFlow::Break`.
    Cancelled,
}

/// Progress after a full, coupled superpixel/annealing iteration.
#[derive(Clone, Debug)]
pub struct Progress {
    /// Iteration number within this call, starting at one.
    pub iteration: usize,
    /// Temperature used for this iteration.
    pub temperature: f64,
    /// Number of logical palette colors, excluding paired components.
    pub palette_colors: usize,
    /// Sum of component center displacements.
    pub palette_delta: f64,
    /// Number of input pixels whose superpixel assignment changed.
    pub assignment_changes: usize,
    /// Number of output cells without input pixels in this iteration.
    pub empty_superpixels: usize,
}

/// Run diagnostics. A returned image does not by itself imply convergence.
#[derive(Clone, Debug)]
pub struct Statistics {
    /// Number of complete coupled iterations performed by this call.
    pub iterations: usize,
    /// Temperature used in the last completed iteration.
    pub temperature: f64,
    /// Explicit termination status.
    pub stop_reason: StopReason,
    /// Last palette movement, measured before any final component collapse.
    pub palette_delta: f64,
    /// Empty superpixels in the last iteration.
    pub empty_superpixels: usize,
}

/// Indexed pixel art. Palette entries can coincide after RGB gamut clipping;
/// palette indices stay stable to support edits and constraints.
#[derive(Clone, Debug)]
pub struct PixelArt {
    /// Output width.
    pub width: usize,
    /// Output height.
    pub height: usize,
    /// Display-ready sRGB palette, after optional saturation.
    pub palette: Vec<[u8; 3]>,
    /// Row-major palette indices.
    pub indices: Vec<u8>,
    /// Per-output coverage from the same superpixel partition as the colors.
    pub alpha: Vec<u8>,
    /// Convergence and budget diagnostics.
    pub statistics: Statistics,
}

impl PixelArt {
    /// Expand the indexed result to a full RGB image.
    pub fn to_rgb_image(&self) -> RgbImage {
        RgbImage {
            width: self.width,
            height: self.height,
            alpha: None,
            pixels: self
                .indices
                .iter()
                .map(|&i| self.palette[usize::from(i)])
                .collect(),
        }
    }
}

/// One-shot abstraction. Use [`Pixelator`] to set importance or edit/resume.
pub fn pixelate(input: &RgbImage, options: &Options) -> Result<PixelArt, Error> {
    Ok(Pixelator::new(input, options.clone())?.run())
}

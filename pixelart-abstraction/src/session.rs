use crate::color::{lab_to_srgb, squared_distance, srgb_to_lab, Lab};
use crate::math::{covariance, principal_axis};
use crate::palette::Palette;
use crate::spatial::SpatialIndex;
use crate::{Error, Options, PixelArt, Progress, RgbImage, Statistics, StopReason};
#[cfg(feature = "parallel")]
use rayon::prelude::*;
use std::ops::ControlFlow;

#[derive(Clone, Copy, Default)]
struct Accumulator {
    count: usize,
    coverage: f64,
    color: Lab,
    position: [f64; 2],
    importance: f64,
    changes: usize,
}

/// An optimization session. The input is converted once and working buffers are
/// reused. `run` can continue an interrupted anneal; after initial convergence,
/// palette/pixel edits continue at the final temperature with stable indices.
pub struct Pixelator {
    options: Options,
    input_width: usize,
    input_height: usize,
    input: Vec<Lab>,
    alpha: Option<Vec<f64>>,
    coverage: Vec<f64>,
    importance: Option<Vec<f64>>,
    labels: Vec<usize>,
    centers: Vec<[f64; 2]>,
    next_centers: Vec<[f64; 2]>,
    means: Vec<Lab>,
    filtered: Vec<Lab>,
    priors: Vec<f64>,
    picks: Vec<usize>,
    allowed: Vec<Option<Vec<usize>>>,
    palette: Palette,
    temperature: f64,
    editable: bool,
    chunk_size: usize,
    shards: Vec<Vec<Accumulator>>,
    totals: Vec<Accumulator>,
    kernel: Vec<(isize, isize, f64)>,
}

impl Pixelator {
    /// Initialize regular superpixels and the input-image mean palette (§4.1).
    pub fn new(input: &RgbImage, options: Options) -> Result<Self, Error> {
        options.validate(input)?;
        let n = options.width * options.height;
        let m = input.pixels().len();
        #[cfg(feature = "parallel")]
        let colors: Vec<Lab> = if options.parallel && m >= 16384 {
            input
                .pixels()
                .par_iter()
                .map(|&rgb| srgb_to_lab(rgb))
                .collect()
        } else {
            input.pixels().iter().map(|&rgb| srgb_to_lab(rgb)).collect()
        };
        #[cfg(not(feature = "parallel"))]
        let colors: Vec<Lab> = input.pixels().iter().map(|&rgb| srgb_to_lab(rgb)).collect();
        let alpha: Option<Vec<f64>> = input
            .alpha()
            .map(|values| values.iter().map(|&a| f64::from(a) / 255.0).collect());
        if alpha
            .as_ref()
            .is_some_and(|values| values.iter().all(|&a| a == 0.0))
        {
            return Err(Error::InvalidImage(
                "fully transparent images have no colors to optimize".into(),
            ));
        }
        let (cov, mean) = covariance(&colors, alpha.as_deref());
        let (variance, axis) = principal_axis(cov);
        let temperature = (2.2 * variance).max(options.final_temperature);
        let palette = Palette::new(mean, axis, options.colors.min(n), options.perturbation);
        let sx = input.width() as f64 / options.width as f64;
        let sy = input.height() as f64 / options.height as f64;
        let centers: Vec<_> = (0..n)
            .map(|s| {
                [
                    (s % options.width) as f64 * sx + (sx - 1.0) * 0.5,
                    (s / options.width) as f64 * sy + (sy - 1.0) * 0.5,
                ]
            })
            .collect();
        // Nearest regular center, with the lower grid index winning exact ties.
        let labels: Vec<_> = (0..m)
            .map(|i| {
                let x = ((((i % input.width()) as f64 + 0.5) / sx).ceil() as usize)
                    .saturating_sub(1)
                    .min(options.width - 1);
                let y = ((((i / input.width()) as f64 + 0.5) / sy).ceil() as usize)
                    .saturating_sub(1)
                    .min(options.height - 1);
                y * options.width + x
            })
            .collect();
        // At most 32 deterministic shards, with at least 4K input pixels each.
        // Their boundaries do not depend on the Rayon thread count.
        let chunk_size = m.div_ceil(32).max(4096);
        let shards = vec![vec![Accumulator::default(); n]; m.div_ceil(chunk_size)];
        let r = options.bilateral.radius as isize;
        let mut kernel = Vec::new();
        for dy in -r..=r {
            for dx in -r..=r {
                let spatial = (-((dx * dx + dy * dy) as f64)
                    / (2.0 * options.bilateral.spatial_sigma.powi(2)))
                .exp();
                kernel.push((dx, dy, spatial));
            }
        }
        Ok(Self {
            options,
            input_width: input.width(),
            input_height: input.height(),
            input: colors,
            alpha,
            coverage: vec![1.0; n],
            importance: None,
            labels,
            next_centers: centers.clone(),
            centers,
            means: vec![mean; n],
            filtered: vec![mean; n],
            priors: vec![1.0 / n as f64; n],
            picks: vec![0; n],
            allowed: vec![None; n],
            palette,
            temperature,
            editable: false,
            chunk_size,
            shards,
            totals: vec![Accumulator::default(); n],
            kernel,
        })
    }

    /// Set or clear the input importance map (§5.1). Values must be finite in
    /// [0,1], match the input, and contain at least one positive value.
    /// Positive maps are rescaled by their maximum before aggregation; this
    /// preserves Eq. (5) while supporting very small positive weights safely.
    pub fn set_importance_map(&mut self, weights: Option<&[f64]>) -> Result<(), Error> {
        match weights {
            None => self.importance = None,
            Some(weights) => {
                if weights.len() != self.input.len()
                    || weights
                        .iter()
                        .any(|w| !w.is_finite() || !(0.0..=1.0).contains(w))
                {
                    return Err(Error::InvalidImportance(
                        "expected one finite [0,1] value per input pixel".into(),
                    ));
                }
                let maximum = weights.iter().copied().fold(0.0_f64, f64::max);
                if maximum == 0.0 {
                    return Err(Error::InvalidImportance(
                        "all-zero maps have no defined normalized prior".into(),
                    ));
                }
                self.importance = Some(weights.iter().map(|w| w / maximum).collect());
            }
        }
        Ok(())
    }

    /// Run or continue optimization with the configured iteration budget.
    /// Always inspect `result.statistics.stop_reason` when convergence matters.
    pub fn run(&mut self) -> PixelArt {
        self.run_with_progress(|_| ControlFlow::Continue(()))
    }

    /// Run with a callback after each complete coupled iteration. A `Break(())`
    /// cancels cooperatively and keeps all state so a later `run` can continue.
    pub fn run_with_progress(
        &mut self,
        mut callback: impl FnMut(&Progress) -> ControlFlow<()>,
    ) -> PixelArt {
        let mut statistics = Statistics {
            iterations: 0,
            temperature: self.temperature,
            stop_reason: StopReason::IterationLimit,
            palette_delta: f64::INFINITY,
            empty_superpixels: 0,
        };
        for iteration in 1..=self.options.max_iterations {
            let (changes, empty) = self.refine_superpixels();
            self.filter_means();
            self.associate();
            let delta = self
                .palette
                .refine(&self.filtered, &self.priors, self.options.parallel);
            self.palette.hard_assignments(&mut self.picks);
            statistics.iterations = iteration;
            statistics.temperature = self.temperature;
            statistics.palette_delta = delta;
            statistics.empty_superpixels = empty;
            let progress = Progress {
                iteration,
                temperature: self.temperature,
                palette_colors: self.palette.len(),
                palette_delta: delta,
                assignment_changes: changes,
                empty_superpixels: empty,
            };
            if delta <= self.options.palette_tolerance {
                let at_final = self.temperature <= self.options.final_temperature;
                if !at_final {
                    self.temperature = (self.temperature * self.options.cooling_rate)
                        .max(self.options.final_temperature);
                }
                if self.palette.paired {
                    let added = self
                        .palette
                        .expand(&self.filtered, &self.priors, &self.options);
                    if at_final && added == 0 {
                        self.palette.collapse();
                    }
                    // Component layout changed. Refresh probabilities and logical
                    // assignments before another segmentation or expansion step.
                    self.associate();
                    self.palette.hard_assignments(&mut self.picks);
                    // Even a no-split final collapse gets a complete flat-palette
                    // iteration at Tf before we can report convergence.
                } else if at_final {
                    self.editable = true;
                    statistics.stop_reason = StopReason::Converged;
                }
            }
            // Cancellation is a boundary between *complete* iterations, including
            // cooling/expansion. Resuming must not repeat a pending transition.
            let cancelled = callback(&progress).is_break();
            if statistics.stop_reason == StopReason::Converged {
                break;
            }
            if cancelled {
                statistics.stop_reason = StopReason::Cancelled;
                break;
            }
        }
        // Refine changed the palette after its E-step. Display the maximum
        // posterior under the current colors, not stale pre-update colors.
        // Keep these display picks separate: an intermediate preview must not
        // change the state that the next segmentation step would have consumed.
        self.associate();
        let mut display_picks = vec![0; self.picks.len()];
        self.palette.hard_assignments(&mut display_picks);
        let palette = self
            .palette
            .colors()
            .iter()
            .enumerate()
            .map(|(k, &lab)| {
                if let Some(rgb) = self.palette.exact_rgb[k] {
                    return rgb;
                }
                lab_to_srgb([
                    lab[0],
                    lab[1] * self.options.saturation,
                    lab[2] * self.options.saturation,
                ])
            })
            .collect();
        PixelArt {
            width: self.options.width,
            height: self.options.height,
            palette,
            indices: display_picks.iter().map(|&i| i as u8).collect(),
            alpha: self
                .coverage
                .iter()
                .map(|a| (a * 255.0).round().clamp(0.0, 255.0) as u8)
                .collect(),
            statistics,
        }
    }

    /// Whether the initial anneal has converged and palette indices can be edited.
    pub fn is_editable(&self) -> bool {
        self.editable
    }

    /// Immutable configuration for this session.
    pub fn options(&self) -> &Options {
        &self.options
    }

    /// Unsaturated logical palette colors. Returning a copy keeps invariants intact.
    pub fn palette_lab(&self) -> Vec<Lab> {
        self.palette.colors()
    }

    /// The current partition: one output-grid index for every input pixel.
    pub fn input_labels(&self) -> &[usize] {
        &self.labels
    }

    /// Centers after four-neighbor smoothing, used by the next segmentation step.
    pub fn superpixel_centers(&self) -> &[[f64; 2]] {
        &self.centers
    }

    /// Raw region means before bilateral filtering, used by palette sampling.
    pub fn superpixel_means(&self) -> &[Lab] {
        &self.means
    }

    /// Normalized region priors from the last completed segmentation step.
    pub fn superpixel_priors(&self) -> &[f64] {
        &self.priors
    }

    /// Restrict an output pixel to existing palette indices (§5.2). Empty sets,
    /// invalid coordinates, and unknown colors are rejected without state changes.
    pub fn set_pixel_constraint(
        &mut self,
        x: usize,
        y: usize,
        allowed: &[usize],
    ) -> Result<(), Error> {
        let s = self.editable_pixel(x, y)?;
        if allowed.is_empty() || allowed.iter().any(|&k| k >= self.palette.len()) {
            return Err(Error::InvalidConstraint(
                "allowed colors must be nonempty and within the current palette".into(),
            ));
        }
        let mut sorted = allowed.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        self.allowed[s] = Some(sorted);
        self.refresh_after_edit();
        Ok(())
    }

    /// Remove a restriction from one output pixel.
    pub fn clear_pixel_constraint(&mut self, x: usize, y: usize) -> Result<(), Error> {
        let s = self.editable_pixel(x, y)?;
        self.allowed[s] = None;
        self.refresh_after_edit();
        Ok(())
    }

    /// Edit an existing palette entry (§5.3). A locked RGB value remains **exact**
    /// in rendered output, bypassing saturation as well as palette updates.
    pub fn edit_palette_color(
        &mut self,
        index: usize,
        rgb: [u8; 3],
        locked: bool,
    ) -> Result<(), Error> {
        self.editable_color(index)?;
        self.palette.clusters[index].color = srgb_to_lab(rgb);
        self.palette.fixed[index] = locked;
        self.palette.exact_rgb[index] = locked.then_some(rgb);
        self.refresh_after_edit();
        Ok(())
    }

    /// Replace a palette entry with the raw Lab mean of an output pixel's region.
    /// Locked sampled colors bypass saturation; their final RGB conversion clips
    /// to the representable gamut once, without changing the optimization color.
    pub fn sample_palette_color(
        &mut self,
        index: usize,
        x: usize,
        y: usize,
        locked: bool,
    ) -> Result<(), Error> {
        self.editable_color(index)?;
        let s = self.editable_pixel(x, y)?;
        self.palette.clusters[index].color = self.means[s];
        self.palette.fixed[index] = locked;
        self.palette.exact_rgb[index] = locked.then(|| lab_to_srgb(self.means[s]));
        self.refresh_after_edit();
        Ok(())
    }

    /// Release a fixed palette color, allowing subsequent refinement and saturation.
    pub fn unlock_palette_color(&mut self, index: usize) -> Result<(), Error> {
        self.editable_color(index)?;
        self.palette.fixed[index] = false;
        self.palette.exact_rgb[index] = None;
        Ok(())
    }

    fn editable_pixel(&self, x: usize, y: usize) -> Result<usize, Error> {
        if !self.editable {
            return Err(Error::NotYetRun);
        }
        if x >= self.options.width || y >= self.options.height {
            return Err(Error::InvalidConstraint(
                "pixel coordinates are outside the output".into(),
            ));
        }
        Ok(y * self.options.width + x)
    }

    fn editable_color(&self, index: usize) -> Result<(), Error> {
        if !self.editable {
            return Err(Error::NotYetRun);
        }
        if index >= self.palette.len() {
            return Err(Error::InvalidConstraint(
                "palette index is out of range".into(),
            ));
        }
        Ok(())
    }

    fn refresh_after_edit(&mut self) {
        // Edits are possible only after initial convergence, so no pair expansion
        // or annealing restart is needed. Do not saturate the stored Lab palette.
        self.temperature = self.options.final_temperature;
        self.associate();
        self.palette.hard_assignments(&mut self.picks);
    }

    fn associate(&mut self) {
        self.palette.associate(
            &self.filtered,
            &self.allowed,
            self.temperature,
            self.options.annealing_distance,
            self.options.parallel,
        );
    }

    fn refine_superpixels(&mut self) -> (usize, usize) {
        let palette = self.palette.colors();
        let representatives: Vec<_> = self.picks.iter().map(|&k| palette[k]).collect();
        let spatial_weight =
            self.options.compactness * (self.centers.len() as f64 / self.input.len() as f64).sqrt();
        let index = SpatialIndex::new(&self.centers, &representatives, spatial_weight);
        let input = &self.input;
        let importance = &self.importance;
        let alpha = &self.alpha;
        let width = self.input_width;
        let chunk_size = self.chunk_size;
        let process = |chunk: usize, labels: &mut [usize], sums: &mut [Accumulator]| {
            sums.fill(Accumulator::default());
            for (offset, label) in labels.iter_mut().enumerate() {
                let i = chunk * chunk_size + offset;
                let position = [(i % width) as f64, (i / width) as f64];
                let opacity = alpha.as_ref().map_or(1.0, |values| values[i]);
                // Hidden RGB cannot move a transparent sample to a colored region.
                let chosen = index.nearest(position, input[i], *label, opacity);
                let sum = &mut sums[chosen];
                sum.changes += usize::from(chosen != *label);
                *label = chosen;
                sum.count += 1;
                sum.coverage += opacity;
                for (channel, &value) in sum.color.iter_mut().zip(&input[i]) {
                    *channel += value * opacity;
                }
                sum.position[0] += position[0];
                sum.position[1] += position[1];
                if let Some(importance) = importance {
                    sum.importance += importance[i] * opacity;
                }
            }
        };
        #[cfg(feature = "parallel")]
        if self.options.parallel && self.shards.len() > 1 {
            self.labels
                .par_chunks_mut(chunk_size)
                .zip(self.shards.par_iter_mut())
                .enumerate()
                .for_each(|(chunk, (labels, sums))| process(chunk, labels, sums));
        } else {
            self.labels
                .chunks_mut(chunk_size)
                .zip(self.shards.iter_mut())
                .enumerate()
                .for_each(|(chunk, (labels, sums))| process(chunk, labels, sums));
        }
        #[cfg(not(feature = "parallel"))]
        self.labels
            .chunks_mut(chunk_size)
            .zip(self.shards.iter_mut())
            .enumerate()
            .for_each(|(chunk, (labels, sums))| process(chunk, labels, sums));
        self.totals.fill(Accumulator::default());
        for sums in &self.shards {
            for (total, sum) in self.totals.iter_mut().zip(sums) {
                total.count += sum.count;
                total.coverage += sum.coverage;
                total.changes += sum.changes;
                for a in 0..3 {
                    total.color[a] += sum.color[a];
                }
                for a in 0..2 {
                    total.position[a] += sum.position[a];
                }
                total.importance += sum.importance;
            }
        }
        let mut changes = 0;
        let mut empty = 0;
        for (s, total) in self.totals.iter().enumerate() {
            changes += total.changes;
            if total.count == 0 {
                empty += 1;
                self.coverage[s] = if self.alpha.is_some() { 0.0 } else { 1.0 };
                continue;
            }
            let n = total.count as f64;
            self.centers[s] = [total.position[0] / n, total.position[1] / n];
            self.coverage[s] = total.coverage / n;
            if total.coverage > 0.0 {
                self.means[s] = std::array::from_fn(|a| total.color[a] / total.coverage);
            }
        }
        // Empty cells retain their previous mean/center. No fabricated input
        // pixels or divisions by zero. With importance they receive zero prior.
        normalize_priors(
            &self.totals,
            self.importance.is_some(),
            self.alpha.is_some(),
            &mut self.priors,
        );
        smooth_positions(
            &self.centers,
            &mut self.next_centers,
            self.options.width,
            self.options.height,
            self.options.position_smoothing,
        );
        std::mem::swap(&mut self.centers, &mut self.next_centers);
        debug_assert!(self.centers.iter().all(|p| p[0] >= 0.0
            && p[0] <= (self.input_width - 1) as f64 + 1e-9
            && p[1] >= 0.0
            && p[1] <= (self.input_height - 1) as f64 + 1e-9));
        (changes, empty)
    }

    fn filter_means(&mut self) {
        let width = self.options.width;
        let height = self.options.height;
        let denominator = 2.0 * self.options.bilateral.color_sigma.powi(2);
        let means = &self.means;
        let kernel = &self.kernel;
        let coverage = &self.coverage;
        let filter = |s: usize, output: &mut Lab| {
            if coverage[s] == 0.0 {
                *output = means[s];
                return;
            }
            let x = (s % width) as isize;
            let y = (s / width) as isize;
            let mut weight_sum = 0.0;
            let mut sum = [0.0; 3];
            for &(dx, dy, spatial) in kernel {
                let nx = x + dx;
                let ny = y + dy;
                if nx < 0 || ny < 0 || nx >= width as isize || ny >= height as isize {
                    continue;
                }
                let neighbor = means[ny as usize * width + nx as usize];
                let weight = coverage[ny as usize * width + nx as usize]
                    * spatial
                    * (-squared_distance(means[s], neighbor) / denominator).exp();
                weight_sum += weight;
                for a in 0..3 {
                    sum[a] += weight * neighbor[a];
                }
            }
            *output = std::array::from_fn(|a| sum[a] / weight_sum);
        };
        #[cfg(feature = "parallel")]
        if self.options.parallel && means.len() >= 1024 {
            self.filtered
                .par_iter_mut()
                .enumerate()
                .for_each(|(s, c)| filter(s, c));
            return;
        }
        self.filtered
            .iter_mut()
            .enumerate()
            .for_each(|(s, c)| filter(s, c));
    }
}

fn normalize_priors(totals: &[Accumulator], importance: bool, alpha: bool, priors: &mut [f64]) {
    if !importance && !alpha {
        priors.fill(1.0 / totals.len() as f64);
        return;
    }
    let mut sum = 0.0;
    for (total, prior) in totals.iter().zip(priors.iter_mut()) {
        *prior = if total.count == 0 {
            0.0
        } else {
            (if importance {
                total.importance
            } else {
                total.coverage
            }) / total.count as f64
        };
        sum += *prior;
    }
    // A map can put all importance on invisible pixels. Use visible coverage then.
    if sum == 0.0 {
        for (total, prior) in totals.iter().zip(priors.iter_mut()) {
            *prior = if total.count == 0 {
                0.0
            } else {
                total.coverage / total.count as f64
            };
        }
        sum = priors.iter().sum();
    }
    for prior in priors {
        *prior /= sum;
    }
}

fn smooth_positions(
    centers: &[[f64; 2]],
    output: &mut [[f64; 2]],
    width: usize,
    height: usize,
    amount: f64,
) {
    for (s, next) in output.iter_mut().enumerate() {
        let x = s % width;
        let y = s / width;
        let mut total = [0.0; 2];
        let mut count = 0;
        for neighbor in [
            (x > 0).then(|| s - 1),
            (x + 1 < width).then_some(s + 1),
            (y > 0).then(|| s - width),
            (y + 1 < height).then_some(s + width),
        ]
        .into_iter()
        .flatten()
        {
            total[0] += centers[neighbor][0];
            total[1] += centers[neighbor][1];
            count += 1;
        }
        *next = if count == 0 {
            centers[s]
        } else {
            std::array::from_fn(|a| {
                (1.0 - amount) * centers[s][a] + amount * total[a] / count as f64
            })
        };
    }
}

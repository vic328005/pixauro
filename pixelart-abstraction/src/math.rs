//! Small deterministic numerical routines for color-space covariance matrices.

/// Returns the largest eigenvalue and a unit principal axis of a finite,
/// symmetric positive-semidefinite 3-by-3 covariance matrix.
///
/// A scaled Jacobi diagonalization avoids both overflow in rotations and the
/// orthogonal-initial-vector failure of power iteration. At a repeated largest
/// eigenvalue, the axis is the first nonzero projection of the coordinate axes
/// onto the largest eigenspace (x, then y, then z). The largest-magnitude axis
/// component is made nonnegative, breaking exact component ties by index.
/// Eigenvalues equal within numerical tolerance are treated as repeated.
/// The zero matrix returns `(0, [1, 0, 0])`.
pub(crate) fn principal_axis(covariance: [[f64; 3]; 3]) -> (f64, [f64; 3]) {
    debug_assert!(covariance.iter().flatten().all(|x| x.is_finite()));
    let scale = covariance
        .iter()
        .flatten()
        .fold(0.0_f64, |max, value| max.max(value.abs()));
    if scale == 0.0 {
        return (0.0, [1.0, 0.0, 0.0]);
    }

    let mut a = covariance.map(|row| row.map(|value| value / scale));
    for (p, q) in [(0, 1), (0, 2), (1, 2)] {
        // Average after scaling, so two large finite entries cannot overflow.
        let value = 0.5 * a[p][q] + 0.5 * a[q][p];
        a[p][q] = value;
        a[q][p] = value;
    }
    let mut vectors = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    // Three-dimensional Jacobi converges well before this conservative limit.
    for _ in 0..64 {
        let (mut p, mut q) = (0, 1);
        for (i, j) in [(0, 2), (1, 2)] {
            if a[i][j].abs() > a[p][q].abs() {
                (p, q) = (i, j);
            }
        }
        if a[p][q].abs() <= 4.0 * f64::EPSILON {
            break;
        }
        let apq = a[p][q];
        let half_difference = 0.5 * (a[q][q] - a[p][p]);
        // The usual tau=(aqq-app)/(2*apq) can overflow when apq is tiny.
        let t = if half_difference == 0.0 {
            apq.signum()
        } else {
            apq / (half_difference + half_difference.hypot(apq).copysign(half_difference))
        };
        let c = 1.0 / (1.0 + t * t).sqrt();
        let s = t * c;
        a[p][p] -= t * apq;
        a[q][q] += t * apq;
        a[p][q] = 0.0;
        a[q][p] = 0.0;
        let k = 3 - p - q; // The only remaining axis in a 3-by-3 matrix.
        let akp = a[k][p];
        let akq = a[k][q];
        a[k][p] = c * akp - s * akq;
        a[p][k] = a[k][p];
        a[k][q] = s * akp + c * akq;
        a[q][k] = a[k][q];
        for row in &mut vectors {
            let vp = row[p];
            let vq = row[q];
            row[p] = c * vp - s * vq;
            row[q] = s * vp + c * vq;
        }
    }

    let largest = a[0][0].max(a[1][1]).max(a[2][2]);
    let repeated_tolerance = 16.0 * f64::EPSILON * largest.abs().max(1.0);
    let top = std::array::from_fn::<_, 3, _>(|j| largest - a[j][j] <= repeated_tolerance);
    let mut axis = [0.0; 3];
    for coordinate in 0..3 {
        axis.fill(0.0);
        for j in 0..3 {
            if top[j] {
                for i in 0..3 {
                    axis[i] += vectors[i][j] * vectors[coordinate][j];
                }
            }
        }
        let norm = axis[0].hypot(axis[1]).hypot(axis[2]);
        if norm > 32.0 * f64::EPSILON {
            for value in &mut axis {
                *value /= norm;
            }
            break;
        }
    }

    let mut sign_index = 0;
    for i in 1..3 {
        if axis[i].abs() > axis[sign_index].abs() {
            sign_index = i;
        }
    }
    if axis[sign_index] < 0.0 {
        for value in &mut axis {
            *value = -*value;
        }
    }
    for value in &mut axis {
        if *value == 0.0 {
            *value = 0.0; // Canonicalize negative zero as well as the axis sign.
        }
    }
    (largest.max(0.0) * scale, axis)
}

#[derive(Clone, Copy, Default)]
struct CompensatedSum {
    sum: f64,
    correction: f64,
}

impl CompensatedSum {
    fn add(&mut self, value: f64) {
        let corrected = value - self.correction;
        let next = self.sum + corrected;
        self.correction = (next - self.sum) - corrected;
        self.sum = next;
    }
}

/// Computes a weighted mean and **population** covariance (division by total
/// weight, not `n - 1`), returning `(covariance, mean)`.
///
/// `None` means equal weights. Empty input or all-zero weights returns zeros.
/// Zero-weight samples are ignored. Samples must be finite, and explicit
/// weights must be finite, nonnegative and match the number of samples.
/// Weights are rescaled before summing, avoiding overflow of their raw sum.
/// A compensated two-pass calculation avoids subtracting large raw moments.
///
/// # Panics
///
/// Panics if an explicit weight is invalid or the weight slice length differs.
pub(crate) fn covariance(
    samples: &[[f64; 3]],
    weights: Option<&[f64]>,
) -> ([[f64; 3]; 3], [f64; 3]) {
    debug_assert!(samples.iter().flatten().all(|x| x.is_finite()));
    if let Some(weights) = weights {
        assert_eq!(
            samples.len(),
            weights.len(),
            "sample and weight lengths must match"
        );
        assert!(
            weights.iter().all(|w| w.is_finite() && *w >= 0.0),
            "weights must be finite and nonnegative"
        );
    }
    let weight_scale = weights.map_or(1.0, |ws| ws.iter().copied().fold(0.0_f64, f64::max));
    if samples.is_empty() || weight_scale == 0.0 {
        return ([[0.0; 3]; 3], [0.0; 3]);
    }
    let weight_at = |i: usize| weights.map_or(1.0, |ws| ws[i] / weight_scale);
    let mut total = CompensatedSum::default();
    for i in 0..samples.len() {
        total.add(weight_at(i));
    }
    let mut mean_sums = [CompensatedSum::default(); 3];
    for (i, sample) in samples.iter().enumerate() {
        let weight = weight_at(i) / total.sum;
        if weight == 0.0 {
            continue;
        }
        for j in 0..3 {
            mean_sums[j].add(weight * sample[j]);
        }
    }
    let mean = mean_sums.map(|sum| sum.sum);
    let mut moments = [[CompensatedSum::default(); 3]; 3];
    for (i, sample) in samples.iter().enumerate() {
        let weight = weight_at(i) / total.sum;
        if weight == 0.0 {
            continue;
        }
        let delta = std::array::from_fn::<_, 3, _>(|j| sample[j] - mean[j]);
        for p in 0..3 {
            for q in p..3 {
                moments[p][q].add(weight * delta[p] * delta[q]);
            }
        }
    }
    let mut result = [[0.0; 3]; 3];
    for p in 0..3 {
        for q in p..3 {
            result[p][q] = moments[p][q].sum;
            result[q][p] = result[p][q];
        }
    }
    (result, mean)
}

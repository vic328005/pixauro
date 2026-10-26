use crate::color::{distance, squared_distance, Lab};
use crate::math::principal_axis;
use crate::{AnnealingDistance, Options};
#[cfg(feature = "parallel")]
use rayon::prelude::*;

#[derive(Clone, Debug)]
pub(crate) struct Cluster {
    pub color: Lab,
    pub mass: f64,
}

pub(crate) struct Palette {
    pub clusters: Vec<Cluster>,
    pub paired: bool,
    pub fixed: Vec<bool>,
    pub exact_rgb: Vec<Option<[u8; 3]>>,
    pub probabilities: Vec<f64>,
    log_masses: Vec<f64>,
    target: usize,
}

impl Palette {
    pub fn new(mean: Lab, axis: Lab, target: usize, perturbation: f64) -> Self {
        let paired = target > 1;
        let clusters = if paired {
            vec![
                Cluster {
                    color: offset(mean, axis, -perturbation),
                    mass: 0.5,
                },
                Cluster {
                    color: offset(mean, axis, perturbation),
                    mass: 0.5,
                },
            ]
        } else {
            vec![Cluster {
                color: mean,
                mass: 1.0,
            }]
        };
        Self {
            clusters,
            paired,
            fixed: vec![false],
            exact_rgb: vec![None],
            probabilities: Vec::new(),
            log_masses: Vec::new(),
            target,
        }
    }

    pub fn len(&self) -> usize {
        self.clusters.len() / if self.paired { 2 } else { 1 }
    }

    pub fn colors(&self) -> Vec<Lab> {
        if self.paired {
            self.clusters
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| average(p[0].color, p[1].color))
                .collect()
        } else {
            self.clusters.iter().map(|c| c.color).collect()
        }
    }

    /// Eq. (2), normalized using log-sum-exp. The 1e-300 mass floor allows
    /// explicitly constrained, formerly unused colors to become active again.
    pub fn associate(
        &mut self,
        samples: &[Lab],
        allowed: &[Option<Vec<usize>>],
        temperature: f64,
        metric: AnnealingDistance,
        parallel: bool,
    ) {
        let count = self.clusters.len();
        self.probabilities.resize(samples.len() * count, 0.0);
        self.log_masses.clear();
        self.log_masses
            .extend(self.clusters.iter().map(|c| c.mass.max(1e-300).ln()));
        let clusters = &self.clusters;
        let masses = &self.log_masses;
        let pair_size = if self.paired { 2 } else { 1 };
        let compute = |s: usize, row: &mut [f64]| {
            let mut maximum = f64::NEG_INFINITY;
            for (k, value) in row.iter_mut().enumerate() {
                if allowed[s]
                    .as_ref()
                    .is_some_and(|a| a.binary_search(&(k / pair_size)).is_err())
                {
                    *value = f64::NEG_INFINITY;
                    continue;
                }
                let d = match metric {
                    AnnealingDistance::Euclidean => distance(samples[s], clusters[k].color),
                    AnnealingDistance::SquaredEuclidean => {
                        squared_distance(samples[s], clusters[k].color)
                    }
                };
                *value = masses[k] - d / temperature;
                maximum = maximum.max(*value);
            }
            let mut sum = 0.0;
            for value in row.iter_mut() {
                *value = (*value - maximum).exp();
                sum += *value;
            }
            for value in row {
                *value /= sum;
            }
        };
        #[cfg(feature = "parallel")]
        if parallel && samples.len() * count >= 8192 {
            self.probabilities
                .par_chunks_mut(count)
                .enumerate()
                .for_each(|(s, row)| compute(s, row));
            return;
        }
        let _ = parallel;
        self.probabilities
            .chunks_mut(count)
            .enumerate()
            .for_each(|(s, row)| compute(s, row));
    }

    /// Eqs. (3–4). Each component accumulates in sample index order, independent
    /// of Rayon scheduling; components themselves can be processed in parallel.
    pub fn refine(&mut self, samples: &[Lab], priors: &[f64], parallel: bool) -> f64 {
        let count = self.clusters.len();
        let probabilities = &self.probabilities;
        let pair_size = if self.paired { 2 } else { 1 };
        let fixed = &self.fixed;
        let update = |k: usize, cluster: &mut Cluster| {
            let mut mass = 0.0;
            let mut total = [0.0; 3];
            for s in 0..samples.len() {
                let w = priors[s] * probabilities[s * count + k];
                mass += w;
                for (a, t) in total.iter_mut().enumerate() {
                    *t += w * samples[s][a];
                }
            }
            cluster.mass = mass;
            if mass <= 1e-300 || fixed[k / pair_size] {
                return 0.0;
            }
            for t in &mut total {
                *t /= mass;
            }
            let delta = distance(cluster.color, total);
            cluster.color = total;
            delta
        };
        // Collect in component order before summing: no floating parallel reduction.
        #[cfg(feature = "parallel")]
        if parallel && count >= 4 && samples.len() * count >= 8192 {
            let deltas: Vec<_> = self
                .clusters
                .par_iter_mut()
                .enumerate()
                .map(|(k, c)| update(k, c))
                .collect();
            return deltas.iter().sum();
        }
        let _ = parallel;
        self.clusters
            .iter_mut()
            .enumerate()
            .map(|(k, c)| update(k, c))
            .sum()
    }

    /// Marginalize paired components, then select the most probable logical color.
    pub fn hard_assignments(&self, output: &mut [usize]) {
        let count = self.clusters.len();
        let stride = if self.paired { 2 } else { 1 };
        for (s, pick) in output.iter_mut().enumerate() {
            let row = &self.probabilities[s * count..(s + 1) * count];
            let mut best = f64::NEG_INFINITY;
            *pick = 0;
            for (k, components) in row.chunks_exact(stride).enumerate() {
                let probability: f64 = components.iter().sum();
                if probability > best {
                    best = probability;
                    *pick = k;
                }
            }
        }
    }

    /// Split the most-separated eligible pairs first, preserving existing logical
    /// indices and appending new colors. Return the number of new logical colors.
    pub fn expand(&mut self, samples: &[Lab], priors: &[f64], options: &Options) -> usize {
        if !self.paired {
            return 0;
        }
        let old_count = self.clusters.len();
        let groups = self.len();
        let mut candidates: Vec<_> = (0..groups)
            .map(|g| {
                (
                    g,
                    distance(self.clusters[2 * g].color, self.clusters[2 * g + 1].color),
                )
            })
            .filter(|(_, separation)| *separation > options.split_threshold)
            .collect();
        candidates.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        candidates.truncate(self.target - groups);
        let mut split = vec![false; groups];
        for &(g, _) in &candidates {
            split[g] = true;
        }

        let mut descriptors: Vec<(Cluster, Vec<usize>)> =
            Vec::with_capacity(groups + candidates.len());
        for (g, &is_split) in split.iter().enumerate() {
            let a = &self.clusters[2 * g];
            let b = &self.clusters[2 * g + 1];
            if is_split {
                descriptors.push((a.clone(), vec![2 * g]));
            } else {
                descriptors.push((
                    Cluster {
                        color: average(a.color, b.color),
                        mass: a.mass + b.mass,
                    },
                    vec![2 * g, 2 * g + 1],
                ));
            }
        }
        for &(g, _) in &candidates {
            descriptors.push((self.clusters[2 * g + 1].clone(), vec![2 * g + 1]));
        }

        let new_count = descriptors.len();
        self.fixed.resize(new_count, false);
        self.exact_rgb.resize(new_count, None);
        if new_count == self.target {
            self.clusters = descriptors.into_iter().map(|(c, _)| c).collect();
            self.paired = false;
        } else {
            let mut new_clusters = Vec::with_capacity(2 * new_count);
            for (cluster, columns) in descriptors {
                let axis =
                    posterior_axis(samples, priors, &self.probabilities, old_count, &columns);
                new_clusters.push(Cluster {
                    color: offset(cluster.color, axis, -options.perturbation),
                    mass: cluster.mass * 0.5,
                });
                new_clusters.push(Cluster {
                    color: offset(cluster.color, axis, options.perturbation),
                    mass: cluster.mass * 0.5,
                });
            }
            self.clusters = new_clusters;
        }
        candidates.len()
    }

    /// Eliminate component pairs, conserving their total probability mass.
    pub fn collapse(&mut self) {
        if !self.paired {
            return;
        }
        self.clusters = self
            .clusters
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| Cluster {
                color: average(p[0].color, p[1].color),
                mass: p[0].mass + p[1].mass,
            })
            .collect();
        self.paired = false;
    }
}

fn posterior_axis(
    samples: &[Lab],
    priors: &[f64],
    probabilities: &[f64],
    count: usize,
    columns: &[usize],
) -> Lab {
    let weight = |s: usize| {
        priors[s]
            * columns
                .iter()
                .map(|&k| probabilities[s * count + k])
                .sum::<f64>()
    };
    let mut mean = [0.0; 3];
    let mut mass = 0.0;
    for (s, sample) in samples.iter().enumerate() {
        let w = weight(s);
        mass += w;
        for a in 0..3 {
            mean[a] += w * sample[a];
        }
    }
    if mass <= 1e-300 {
        return [1.0, 0.0, 0.0];
    }
    for m in &mut mean {
        *m /= mass;
    }
    let mut cov = [[0.0; 3]; 3];
    for (s, sample) in samples.iter().enumerate() {
        let w = weight(s) / mass;
        for a in 0..3 {
            for b in 0..3 {
                cov[a][b] += w * (sample[a] - mean[a]) * (sample[b] - mean[b]);
            }
        }
    }
    principal_axis(cov).1
}

fn average(a: Lab, b: Lab) -> Lab {
    std::array::from_fn(|i| (a[i] + b[i]) * 0.5)
}
fn offset(color: Lab, axis: Lab, amount: f64) -> Lab {
    std::array::from_fn(|i| color[i] + amount * axis[i])
}

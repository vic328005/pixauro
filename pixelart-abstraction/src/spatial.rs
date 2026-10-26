//! Exact nearest-site queries for the modified SLIC metric.
//!
//! The tree partitions position space, but every node bounds both position and
//! Lab color. Its lower bound is the sum of the two distances to those boxes.
//! Queries use only immutable data and the call stack, so one index can be
//! shared by all Rayon workers without locks or per-query heap allocations.

const LEAF_SIZE: usize = 12;

#[derive(Clone, Copy)]
struct Site {
    position: [f64; 2],
    color: [f64; 3],
}

#[derive(Clone, Copy)]
struct Bounds {
    position_min: [f64; 2],
    position_max: [f64; 2],
    color_min: [f64; 3],
    color_max: [f64; 3],
}

#[derive(Clone, Copy)]
enum Children {
    Leaf { start: usize, len: usize },
    Branch { left: usize, right: usize },
}

struct Node {
    bounds: Bounds,
    // Besides deterministic ties, this permits early exits for identical sites.
    min_index: usize,
    children: Children,
}

#[derive(Clone, Copy)]
struct Best {
    distance: f64,
    index: usize,
}

/// A balanced, immutable spatial tree for
/// `|query_color - site_color| + spatial_weight * |query_position - center|`.
///
/// All coordinates must be finite, and `spatial_weight` must be nonnegative and
/// finite. The caller validates these conditions before building the index.
/// Returned indices always refer to the original input order.
pub(crate) struct SpatialIndex {
    sites: Vec<Site>,
    indices: Vec<usize>,
    nodes: Vec<Node>,
    spatial_weight: f64,
}

impl SpatialIndex {
    pub(crate) fn new(centers: &[[f64; 2]], colors: &[[f64; 3]], spatial_weight: f64) -> Self {
        assert!(!centers.is_empty(), "the spatial index requires a site");
        assert_eq!(centers.len(), colors.len(), "site array lengths differ");
        assert!(
            spatial_weight.is_finite() && spatial_weight >= 0.0,
            "the spatial weight must be nonnegative and finite"
        );
        debug_assert!(centers.iter().flatten().all(|value| value.is_finite()));
        debug_assert!(colors.iter().flatten().all(|value| value.is_finite()));

        let sites: Vec<_> = centers
            .iter()
            .zip(colors)
            .map(|(&position, &color)| Site { position, color })
            .collect();
        let mut indices: Vec<_> = (0..sites.len()).collect();
        // A median split leaves at least LEAF_SIZE / 2 sites in each leaf,
        // except when the entire tree fits in one leaf.
        let mut nodes = Vec::with_capacity(sites.len().saturating_mul(4) / LEAF_SIZE + 1);
        build_node(&sites, &mut indices, 0, &mut nodes);

        Self {
            sites,
            indices,
            nodes,
            spatial_weight,
        }
    }

    /// Finds the exact minimum, resolving equal floating-point distances by the
    /// lowest original site index. `seed` is a valid site index, usually the
    /// previous iteration's assignment; it supplies an initial upper bound.
    pub(crate) fn nearest(
        &self,
        position: [f64; 2],
        color: [f64; 3],
        seed: usize,
        color_weight: f64,
    ) -> usize {
        debug_assert!(position.iter().all(|value| value.is_finite()));
        debug_assert!(color.iter().all(|value| value.is_finite()));
        debug_assert!(seed < self.sites.len());

        let mut best = Best {
            distance: self.distance_to_site(position, color, seed, color_weight),
            index: seed,
        };
        let root = &self.nodes[0];
        let lower = self.lower_bound(&root.bounds, position, color, best.distance, color_weight);
        if can_improve(lower, root.min_index, best) {
            self.visit(0, position, color, &mut best, color_weight);
        }
        best.index
    }

    fn visit(
        &self,
        node_index: usize,
        position: [f64; 2],
        color: [f64; 3],
        best: &mut Best,
        color_weight: f64,
    ) {
        match self.nodes[node_index].children {
            Children::Leaf { start, len } => {
                for &index in &self.indices[start..start + len] {
                    let distance = self.distance_to_site(position, color, index, color_weight);
                    if can_improve(distance, index, *best) {
                        *best = Best { distance, index };
                    }
                }
            }
            Children::Branch { left, right } => {
                let left_node = &self.nodes[left];
                let right_node = &self.nodes[right];
                let left_lower = self.lower_bound(
                    &left_node.bounds,
                    position,
                    color,
                    best.distance,
                    color_weight,
                );
                let right_lower = self.lower_bound(
                    &right_node.bounds,
                    position,
                    color,
                    best.distance,
                    color_weight,
                );

                let ((first, first_lower), (second, second_lower)) = if left_lower < right_lower
                    || (left_lower == right_lower && left_node.min_index < right_node.min_index)
                {
                    ((left, left_lower), (right, right_lower))
                } else {
                    ((right, right_lower), (left, left_lower))
                };

                if can_improve(first_lower, self.nodes[first].min_index, *best) {
                    self.visit(first, position, color, best, color_weight);
                }
                // Recheck against the best found in the first subtree. The
                // cached lower bound remains valid as that upper bound shrinks.
                if can_improve(second_lower, self.nodes[second].min_index, *best) {
                    self.visit(second, position, color, best, color_weight);
                }
            }
        }
    }

    #[inline]
    fn distance_to_site(
        &self,
        position: [f64; 2],
        color: [f64; 3],
        index: usize,
        color_weight: f64,
    ) -> f64 {
        let site = self.sites[index];
        let color_distance = norm3([
            color[0] - site.color[0],
            color[1] - site.color[1],
            color[2] - site.color[2],
        ]);
        if self.spatial_weight == 0.0 {
            return color_weight * color_distance;
        }
        let position_distance = norm2([
            position[0] - site.position[0],
            position[1] - site.position[1],
        ]);
        color_weight * color_distance + self.spatial_weight * position_distance
    }

    #[inline]
    fn lower_bound(
        &self,
        bounds: &Bounds,
        position: [f64; 2],
        color: [f64; 3],
        cutoff: f64,
        color_weight: f64,
    ) -> f64 {
        let spatial = if self.spatial_weight == 0.0 {
            // Also avoids 0 * infinity for finite coordinates whose squared
            // difference overflows. Such coordinates are not ordinary pixels,
            // but zero spatial weight should still define a color-only metric.
            0.0
        } else {
            self.spatial_weight
                * norm2([
                    interval_gap(position[0], bounds.position_min[0], bounds.position_max[0]),
                    interval_gap(position[1], bounds.position_min[1], bounds.position_max[1]),
                ])
        };
        if spatial > cutoff {
            // Spatial distance alone already excludes this subtree. This is
            // still a lower bound, so callers need no special-case handling.
            return spatial;
        }
        let color_distance = norm3([
            interval_gap(color[0], bounds.color_min[0], bounds.color_max[0]),
            interval_gap(color[1], bounds.color_min[1], bounds.color_max[1]),
            interval_gap(color[2], bounds.color_min[2], bounds.color_max[2]),
        ]);
        // Use the same order and operations as distance_to_site. Each box gap
        // is <= every site's absolute component difference. Floating-point
        // subtraction, nonnegative squaring/addition, sqrt, and multiplication
        // by a nonnegative weight are monotone, so this also bounds the distance
        // as actually computed in f64. No approximate bound or epsilon is used.
        color_weight * color_distance + spatial
    }
}

fn build_node(sites: &[Site], indices: &mut [usize], start: usize, nodes: &mut Vec<Node>) -> usize {
    let first = sites[indices[0]];
    let mut bounds = Bounds {
        position_min: first.position,
        position_max: first.position,
        color_min: first.color,
        color_max: first.color,
    };
    let mut min_index = indices[0];
    for &index in &indices[1..] {
        let site = sites[index];
        min_index = min_index.min(index);
        for axis in 0..2 {
            bounds.position_min[axis] = bounds.position_min[axis].min(site.position[axis]);
            bounds.position_max[axis] = bounds.position_max[axis].max(site.position[axis]);
        }
        for axis in 0..3 {
            bounds.color_min[axis] = bounds.color_min[axis].min(site.color[axis]);
            bounds.color_max[axis] = bounds.color_max[axis].max(site.color[axis]);
        }
    }
    let node_index = nodes.len();
    nodes.push(Node {
        bounds,
        min_index,
        children: Children::Leaf {
            start,
            len: indices.len(),
        },
    });
    if indices.len() > LEAF_SIZE {
        let x_extent = bounds.position_max[0] - bounds.position_min[0];
        let y_extent = bounds.position_max[1] - bounds.position_min[1];
        let axis = usize::from(y_extent > x_extent);
        let middle = indices.len() / 2;
        indices.select_nth_unstable_by(middle, |&a, &b| {
            sites[a].position[axis]
                .total_cmp(&sites[b].position[axis])
                .then_with(|| a.cmp(&b))
        });
        let (left_indices, right_indices) = indices.split_at_mut(middle);
        let left = build_node(sites, left_indices, start, nodes);
        let right = build_node(sites, right_indices, start + middle, nodes);
        nodes[node_index].children = Children::Branch { left, right };
    }
    node_index
}

#[inline]
fn interval_gap(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min - value
    } else if value > max {
        value - max
    } else {
        0.0
    }
}

#[inline]
fn norm2(value: [f64; 2]) -> f64 {
    (value[0] * value[0] + value[1] * value[1]).sqrt()
}

#[inline]
fn norm3(value: [f64; 3]) -> f64 {
    (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt()
}

#[inline]
fn can_improve(distance: f64, min_index: usize, best: Best) -> bool {
    distance < best.distance || (distance == best.distance && min_index < best.index)
}

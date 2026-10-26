//! 从编辑器迁入的保守切片定位；只移动裁切框，不修改像素或帧顺序。
use super::{ImportSlice, invalid, validate_slices};
use crate::ProjectError;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    ops::ControlFlow,
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum LocateAxis {
    Both,
    X,
    Y,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum LocateIssue {
    Background,
    Resolution,
    Missing,
    Ambiguous,
    Size,
    Blocked,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum LocateStatus {
    Moved,
    Unchanged,
    Review,
    Locked,
    Excluded,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocateItem {
    pub id: String,
    pub x: u32,
    pub y: u32,
    pub status: LocateStatus,
    pub issue: Option<LocateIssue>,
}
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocateRequest {
    pub width: u16,
    pub height: u16,
    pub source_width: u32,
    pub source_height: u32,
    pub rgba: Vec<u8>,
    pub slices: Vec<ImportSlice>,
    pub axis: LocateAxis,
}
#[derive(Clone, Copy)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
impl Bounds {
    fn cx(self) -> f64 {
        self.x + self.width / 2.0
    }
    fn cy(self) -> f64 {
        self.y + self.height / 2.0
    }
    fn intersection(self, b: Self) -> f64 {
        ((self.x + self.width).min(b.x + b.width) - self.x.max(b.x)).max(0.0)
            * ((self.y + self.height).min(b.y + b.height) - self.y.max(b.y)).max(0.0)
    }
}
struct Subject {
    bounds: Bounds,
    area: f64,
}
fn quantile(mut values: Vec<f64>, q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * q).floor() as usize]
}
fn neighbours(i: usize, w: usize, h: usize) -> impl Iterator<Item = usize> {
    let x = i % w;
    let y = i / w;
    (y.saturating_sub(1)..=(y + 1).min(h - 1))
        .flat_map(move |ny| (x.saturating_sub(1)..=(x + 1).min(w - 1)).map(move |nx| ny * w + nx))
}
fn foreground(request: &LocateRequest) -> Option<Vec<u8>> {
    let (w, h) = (usize::from(request.width), usize::from(request.height));
    let rgba = &request.rgba;
    let band = ((w.min(h) as f64 * 0.015).floor() as usize).clamp(1, 8);
    let border: Vec<_> = (0..w * h)
        .filter(|&i| {
            let (x, y) = (i % w, i / w);
            x < band || y < band || x >= w.saturating_sub(band) || y >= h.saturating_sub(band)
        })
        .collect();
    if border.iter().filter(|&&i| rgba[i * 4 + 3] < 32).count() as f64 / border.len() as f64 > 0.6 {
        return Some(
            rgba.as_chunks::<4>()
                .0
                .iter()
                .map(|p| u8::from(p[3] >= 64))
                .collect(),
        );
    }
    let mut histogram = [0_u32; 4096];
    let mut mode = 0;
    for &i in &border {
        let p = &rgba[i * 4..];
        if p[3] < 192 {
            continue;
        }
        let key =
            (usize::from(p[0] >> 4) << 8) | (usize::from(p[1] >> 4) << 4) | usize::from(p[2] >> 4);
        histogram[key] += 1;
        if histogram[key] > histogram[mode] {
            mode = key;
        }
    }
    let distance = |i: usize, color: [f64; 3]| {
        (0..3)
            .map(|c| (f64::from(rgba[i * 4 + c]) - color[c]).abs())
            .fold(0.0, f64::max)
    };
    let approximate = [
        ((mode >> 8) * 16 + 8) as f64,
        (((mode >> 4) & 15) * 16 + 8) as f64,
        ((mode & 15) * 16 + 8) as f64,
    ];
    let samples: Vec<_> = border
        .iter()
        .copied()
        .filter(|&i| rgba[i * 4 + 3] >= 192 && distance(i, approximate) <= 32.0)
        .collect();
    if (samples.len() as f64) < border.len() as f64 * 0.6 {
        return None;
    }
    let color = std::array::from_fn(|c| {
        quantile(
            samples
                .iter()
                .map(|&i| f64::from(rgba[i * 4 + c]))
                .collect(),
            0.5,
        )
    });
    let tolerance = (quantile(samples.iter().map(|&i| distance(i, color)).collect(), 0.9) + 12.0)
        .clamp(24.0, 48.0);
    let mask: Vec<_> = (0..w * h)
        .map(|i| rgba[i * 4 + 3] >= 64 && distance(i, color) > tolerance)
        .collect();
    let mut dilated = vec![false; w * h];
    for (i, &solid) in mask.iter().enumerate() {
        if solid {
            for next in neighbours(i, w, h) {
                dilated[next] = true;
            }
        }
    }
    let closed: Vec<_> = (0..w * h)
        .map(|i| neighbours(i, w, h).all(|n| dilated[n]))
        .collect();
    let mut outside = vec![false; w * h];
    let mut queue = VecDeque::new();
    let visit = |i: usize, outside: &mut Vec<bool>, queue: &mut VecDeque<usize>| {
        if !outside[i] && !closed[i] {
            outside[i] = true;
            queue.push_back(i);
        }
    };
    for x in 0..w {
        visit(x, &mut outside, &mut queue);
        visit((h - 1) * w + x, &mut outside, &mut queue);
    }
    for y in 0..h {
        visit(y * w, &mut outside, &mut queue);
        visit(y * w + w - 1, &mut outside, &mut queue);
    }
    while let Some(i) = queue.pop_front() {
        if i % w > 0 {
            visit(i - 1, &mut outside, &mut queue);
        }
        if i % w + 1 < w {
            visit(i + 1, &mut outside, &mut queue);
        }
        if i >= w {
            visit(i - w, &mut outside, &mut queue);
        }
        if i + w < w * h {
            visit(i + w, &mut outside, &mut queue);
        }
    }
    let area = outside.iter().filter(|&&v| !v).count();
    if area as f64 > (w * h) as f64 * 0.7 {
        None
    } else {
        Some(outside.into_iter().map(|v| u8::from(!v)).collect())
    }
}
fn components(mut mask: Vec<u8>, w: usize, h: usize, min_area: f64) -> Vec<Subject> {
    let mut result = Vec::new();
    let mut queue = Vec::new();
    for start in 0..mask.len() {
        if mask[start] == 0 {
            continue;
        }
        queue.clear();
        queue.push(start);
        mask[start] = 0;
        let (mut head, mut left, mut right, mut top, mut bottom) = (0, w, 0, h, 0);
        while head < queue.len() {
            let i = queue[head];
            head += 1;
            left = left.min(i % w);
            right = right.max(i % w);
            top = top.min(i / w);
            bottom = bottom.max(i / w);
            for next in neighbours(i, w, h) {
                if mask[next] != 0 {
                    mask[next] = 0;
                    queue.push(next);
                }
            }
        }
        if queue.len() as f64 >= min_area {
            result.push(Subject {
                bounds: Bounds {
                    x: left as f64,
                    y: top as f64,
                    width: (right - left + 1) as f64,
                    height: (bottom - top + 1) as f64,
                },
                area: queue.len() as f64,
            });
        }
    }
    result
}
fn fit_line(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    if points.len() < 2 {
        return None;
    }
    let mut slopes = Vec::new();
    for (i, &(x, y)) in points.iter().enumerate() {
        for &(nx, ny) in &points[i + 1..] {
            if x != nx {
                slopes.push((ny - y) / (nx - x));
            }
        }
    }
    if slopes.is_empty() {
        return Some((0.0, quantile(points.iter().map(|p| p.1).collect(), 0.5)));
    }
    let slope = quantile(slopes, 0.5);
    if slope <= 0.0 {
        return None;
    }
    Some((
        slope,
        quantile(points.iter().map(|p| p.1 - p.0 * slope).collect(), 0.5),
    ))
}
fn positions(low: f64, high: f64, preferred: f64, original: f64) -> Vec<f64> {
    if low > high {
        return Vec::new();
    }
    let mut values = vec![preferred, original, low, high, (low + high) / 2.0];
    values.extend((1..8).map(|i| low + (high - low) * f64::from(i) / 8.0));
    let mut result = Vec::new();
    for value in values {
        let n = (value + 0.5).floor().clamp(low, high);
        if !result.contains(&n) {
            result.push(n);
        }
    }
    result
}

/// 回调返回 Break 取消，不返回部分结果。最多处理 1024² 的源预览。
pub fn auto_locate_slices(
    request: &LocateRequest,
    mut progress: impl FnMut(&str) -> ControlFlow<()>,
) -> Result<Option<Vec<LocateItem>>, ProjectError> {
    let (w, h) = (usize::from(request.width), usize::from(request.height));
    if w == 0
        || h == 0
        || w * h > 1024 * 1024
        || request.rgba.len() != w * h * 4
        || request.source_width < u32::from(request.width)
        || request.source_height < u32::from(request.height)
    {
        return Err(invalid("原图定位预览无效"));
    }
    validate_slices(&request.slices, request.source_width, request.source_height)?;
    let slices = &request.slices;
    let mut base: Vec<_> = slices
        .iter()
        .map(|s| LocateItem {
            id: s.id.clone(),
            x: s.x,
            y: s.y,
            status: if !s.enabled {
                LocateStatus::Excluded
            } else if s.auto_locked {
                LocateStatus::Locked
            } else {
                LocateStatus::Review
            },
            issue: if s.enabled && !s.auto_locked {
                Some(LocateIssue::Missing)
            } else {
                None
            },
        })
        .collect();
    let fail = |mut items: Vec<LocateItem>| {
        for item in &mut items {
            if item.status == LocateStatus::Review {
                item.issue = Some(LocateIssue::Background);
            }
        }
        items
    };
    if progress("Estimating background…").is_break() {
        return Ok(None);
    }
    let Some(mask) = foreground(request) else {
        return Ok(Some(fail(base)));
    };
    let sx = w as f64 / f64::from(request.source_width);
    let sy = h as f64 / f64::from(request.source_height);
    let scaled: Vec<_> = slices
        .iter()
        .map(|s| Bounds {
            x: f64::from(s.x) * sx,
            y: f64::from(s.y) * sy,
            width: f64::from(s.width) * sx,
            height: f64::from(s.height) * sy,
        })
        .collect();
    let typical = quantile(scaled.iter().map(|s| s.width * s.height).collect(), 0.5);
    if progress("Finding subjects beyond the current slice edges…").is_break() {
        return Ok(None);
    }
    let subjects = components(mask, w, h, 12.0_f64.max(typical * 0.012));
    if subjects.len() > 1024 {
        return Ok(Some(fail(base)));
    }
    let candidates: Vec<Vec<(usize, f64)>> = scaled
        .iter()
        .map(|s| {
            let mut list: Vec<_> = subjects
                .iter()
                .enumerate()
                .filter_map(|(i, subject)| {
                    let b = subject.bounds;
                    let dx = (b.cx() - s.cx()) / s.width;
                    let dy = (b.cy() - s.cy()) / s.height;
                    (dx.abs() <= 0.7
                        && dy.abs() <= 0.7
                        && b.width >= s.width * 0.18
                        && b.height >= s.height * 0.25
                        && b.width <= s.width * 1.35
                        && b.height <= s.height * 1.35
                        && subject.area >= s.width * s.height * 0.025)
                        .then_some((i, dx * dx + dy * dy))
                })
                .collect();
            list.sort_by(|a, b| a.1.total_cmp(&b.1));
            list
        })
        .collect();
    let owners: Vec<Vec<(usize, f64)>> = (0..subjects.len())
        .map(|index| {
            let mut list: Vec<_> = candidates
                .iter()
                .enumerate()
                .filter_map(|(slice, list)| {
                    list.iter().find(|p| p.0 == index).map(|p| (slice, p.1))
                })
                .collect();
            list.sort_by(|a, b| a.1.total_cmp(&b.1));
            list
        })
        .collect();
    let matches: Vec<_> = candidates
        .iter()
        .enumerate()
        .map(|(i, list)| {
            let best = list.first()?;
            let owner = &owners[best.0];
            if owner.first().is_none_or(|p| p.0 != i)
                || list.get(1).is_some_and(|p| p.1 - best.1 < 0.08)
                || owner.get(1).is_some_and(|p| p.1 - owner[0].1 < 0.08)
            {
                if base[i].status == LocateStatus::Review {
                    base[i].issue = Some(LocateIssue::Ambiguous);
                }
                None
            } else {
                Some(best.0)
            }
        })
        .collect();
    let mut columns = HashMap::new();
    for row in slices.iter().map(|s| s.row).collect::<BTreeSet<_>>() {
        let mut list: Vec<_> = slices.iter().filter(|s| s.row == row).collect();
        list.sort_by(|a, b| {
            (f64::from(a.x) + f64::from(a.width) / 2.0)
                .total_cmp(&(f64::from(b.x) + f64::from(b.width) / 2.0))
        });
        for (column, s) in list.iter().enumerate() {
            columns.insert(&s.id, column as f64);
        }
    }
    if progress("Fitting rows and columns…").is_break() {
        return Ok(None);
    }
    let line_x = fit_line(
        &matches
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.map(|j| (columns[&slices[i].id], subjects[j].bounds.cx() / sx)))
            .collect::<Vec<_>>(),
    );
    let line_y = fit_line(
        &matches
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                m.map(|j| (f64::from(slices[i].row), subjects[j].bounds.cy() / sy))
            })
            .collect::<Vec<_>>(),
    );
    for (i, item) in base.iter_mut().enumerate() {
        if progress("Positioning slices…").is_break() {
            return Ok(None);
        }
        if item.status != LocateStatus::Review {
            continue;
        }
        let Some(matched) = matches[i] else {
            continue;
        };
        let s = &slices[i];
        let subject = &subjects[matched];
        let b = subject.bounds;
        if scaled[i].width < 16.0 || scaled[i].height < 16.0 {
            item.issue = Some(LocateIssue::Resolution);
            continue;
        }
        let target = Bounds {
            x: b.x / sx,
            y: b.y / sy,
            width: b.width / sx,
            height: b.height / sy,
        };
        let (x, y, sw, sh) = (
            f64::from(s.x),
            f64::from(s.y),
            f64::from(s.width),
            f64::from(s.height),
        );
        let margin = (1.0 / sx.min(sy)).max(sw.min(sh) * 0.02);
        let gx = line_x.map_or(target.cx(), |(slope, origin)| {
            origin + columns[&s.id] * slope
        });
        let gy = line_y.map_or(target.cy(), |(slope, origin)| {
            origin + f64::from(s.row) * slope
        });
        let ranges = |padding: f64| {
            [
                0.0_f64
                    .max(x - sw * 0.45)
                    .max(target.x + target.width + padding - sw)
                    .ceil(),
                (f64::from(request.source_width) - sw)
                    .min(x + sw * 0.45)
                    .min(target.x - padding)
                    .floor(),
                0.0_f64
                    .max(y - sh * 0.45)
                    .max(target.y + target.height + padding - sh)
                    .ceil(),
                (f64::from(request.source_height) - sh)
                    .min(y + sh * 0.45)
                    .min(target.y - padding)
                    .floor(),
            ]
        };
        let mut range = ranges(margin);
        if range[0] > range[1] || range[2] > range[3] {
            range = ranges(0.0);
        }
        let [left, right, top, bottom] = range;
        if left > right || top > bottom {
            item.issue = Some(LocateIssue::Size);
            continue;
        }
        let xs = if matches!(request.axis, LocateAxis::Y) {
            if x >= left && x <= right {
                vec![x]
            } else {
                vec![]
            }
        } else {
            positions(left, right, gx - sw / 2.0, x)
        };
        let ys = if matches!(request.axis, LocateAxis::X) {
            if y >= top && y <= bottom {
                vec![y]
            } else {
                vec![]
            }
        } else {
            positions(top, bottom, gy - sh / 2.0, y)
        };
        let mut best: Option<(f64, f64, f64)> = None;
        for &nx in &xs {
            for &ny in &ys {
                let rect = Bounds {
                    x: nx * sx,
                    y: ny * sy,
                    width: sw * sx,
                    height: sh * sy,
                };
                if subjects.iter().enumerate().any(|(j, other)| {
                    j != matched
                        && other.area >= subject.area * 0.08
                        && rect.intersection(other.bounds)
                            > (other.bounds.width * other.bounds.height * 0.05)
                                .min(subject.area * 0.015)
                }) {
                    continue;
                }
                let score = 4.0
                    * (((nx + sw / 2.0 - gx) / sw).powi(2) + ((ny + sh / 2.0 - gy) / sh).powi(2))
                    + ((nx + sw / 2.0 - target.cx()) / sw).powi(2)
                    + ((ny + sh / 2.0 - target.cy()) / sh).powi(2)
                    + 0.2 * (((nx - x) / sw).powi(2) + ((ny - y) / sh).powi(2));
                if best.is_none_or(|p| score < p.2) {
                    best = Some((nx, ny, score));
                }
            }
        }
        if let Some((nx, ny, _)) = best {
            item.x = nx as u32;
            item.y = ny as u32;
            item.issue = None;
            item.status = if item.x == s.x && item.y == s.y {
                LocateStatus::Unchanged
            } else {
                LocateStatus::Moved
            };
        } else {
            item.issue = Some(LocateIssue::Blocked);
        }
    }
    Ok(Some(base))
}

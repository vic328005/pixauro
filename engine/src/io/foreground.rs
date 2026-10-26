//! 最终像素图上的去背：外部背景填充、有限轮廓缺口补障与内部区域保护。
//! 只生成与最终网格一一对应的蒙版，不修改 RGB，也不读取原图蒙版。

fn distance(a: &[u8; 4], b: &[u8; 4]) -> u8 {
    (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0)
}
fn quantile(values: &mut [u8], numerator: usize, denominator: usize) -> u8 {
    values.sort_unstable();
    values[(values.len() - 1) * numerator / denominator]
}

pub(super) fn estimate(width: usize, height: usize, rgba: &[[u8; 4]]) -> Option<Vec<u8>> {
    let area = width * height;
    let band = (width.min(height) / 64).clamp(1, 8);
    let mut border = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if x < band || y < band || x + band >= width || y + band >= height {
                border.push(rgba[y * width + x]);
            }
        }
    }
    // 已有透明背景时保留原 alpha 与内部透明孔洞，不重新猜测遮罩。
    if border.iter().filter(|p| p[3] < 32).count() * 5 > border.len() * 3 {
        return Some(vec![1; area]);
    }
    let mut histogram = [0usize; 4096];
    let mut mode = 0;
    for p in &border {
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
    let coarse = [
        ((mode >> 8) * 16 + 8) as u8,
        (((mode >> 4) & 15) * 16 + 8) as u8,
        ((mode & 15) * 16 + 8) as u8,
        255,
    ];
    let samples: Vec<_> = border
        .iter()
        .filter(|p| p[3] >= 192 && distance(p, &coarse) <= 32)
        .copied()
        .collect();
    if samples.len() * 5 < border.len() * 3 {
        return None;
    }
    let mut background = [0, 0, 0, 255];
    for c in 0..3 {
        background[c] = quantile(&mut samples.iter().map(|p| p[c]).collect::<Vec<_>>(), 1, 2);
    }
    let tolerance = quantile(
        &mut samples
            .iter()
            .map(|p| distance(p, &background))
            .collect::<Vec<_>>(),
        9,
        10,
    )
    .saturating_add(12)
    .clamp(24, 48);
    // 只把像素本身判为屏障，不把反差边缘的背景一侧也标成前景。
    // 否则即使正确识别了轮廓，也会在轮廓外人为保留一圈背景色。
    let barrier: Vec<_> = rgba
        .iter()
        .map(|p| p[3] >= 64 && distance(p, &background) > tolerance)
        .collect();
    let strong: Vec<_> = rgba
        .iter()
        .map(|p| p[3] >= 192 && distance(p, &background) > tolerance.saturating_mul(2))
        .collect();
    let outside = exterior(width, height, &barrier);
    let mut repaired = barrier.clone();
    let mut bridges = vec![false; area];
    // 只在两端都有延续轮廓的短线段上设置虚拟屏障；一次性推导，不迭代膨胀。
    // 沿水平/垂直最多跨两格，斜向最多跨一格。
    for y in 0..height {
        for x in 0..width {
            for (dx, dy) in [(1, 0), (0, 1), (1, 1), (1, -1)] {
                for gap in 1..=if dx != 0 && dy != 0 { 1 } else { 2 } {
                    let x = x as isize;
                    let y = y as isize;
                    let Some(first) = position(x - dx, y - dy, width, height) else {
                        continue;
                    };
                    let Some(last) = position(x + dx * gap, y + dy * gap, width, height) else {
                        continue;
                    };
                    if !strong[first] || !strong[last] {
                        continue;
                    }
                    if !continuation(x - dx, y - dy, -dx, -dy, width, height, &strong)
                        || !continuation(x + dx * gap, y + dy * gap, dx, dy, width, height, &strong)
                    {
                        continue;
                    }
                    let pixels: Vec<_> = (0..gap)
                        .filter_map(|t| position(x + dx * t, y + dy * t, width, height))
                        .collect();
                    if pixels.iter().any(|&i| barrier[i] || rgba[i][3] < 64) {
                        continue;
                    }
                    for i in pixels {
                        repaired[i] = true;
                        bridges[i] = true;
                    }
                }
            }
        }
    }
    let patched_outside = exterior(width, height, &repaired);
    let mut keep: Vec<_> = outside.iter().map(|&p| u8::from(!p)).collect();
    // 新封闭区域须有足够内部面积；细长的腿间缝等不能因为补障就全部变前景。
    let mut pending: Vec<_> = (0..area)
        .map(|i| outside[i] && !patched_outside[i] && !bridges[i])
        .collect();
    for seed in 0..area {
        if !pending[seed] {
            continue;
        }
        let mut region = vec![seed];
        pending[seed] = false;
        let (mut left, mut right, mut top, mut bottom) = (width, 0, height, 0);
        let mut head = 0;
        while head < region.len() {
            let i = region[head];
            head += 1;
            let x = i % width;
            let y = i / width;
            left = left.min(x);
            right = right.max(x);
            top = top.min(y);
            bottom = bottom.max(y);
            for next in neighbours(i, width, height).into_iter().flatten() {
                if pending[next] {
                    pending[next] = false;
                    region.push(next);
                }
            }
        }
        let w = right - left + 1;
        let h = bottom - top + 1;
        if w >= 3 && h >= 3 && region.len() >= 9 && region.len() * 100 >= w * h * 45 {
            for i in region {
                keep[i] = 1;
            }
        }
    }
    // 虚拟补障格本身不继承为前景：不补画 RGB，不制造新的浅色描边。
    let foreground = keep.iter().filter(|&&p| p != 0).count();
    if foreground == 0 || foreground * 10 > area * 8 {
        return None;
    }
    Some(keep)
}

fn position(x: isize, y: isize, width: usize, height: usize) -> Option<usize> {
    (x >= 0 && y >= 0 && x < width as isize && y < height as isize)
        .then(|| y as usize * width + x as usize)
}

fn continuation(
    x: isize,
    y: isize,
    dx: isize,
    dy: isize,
    width: usize,
    height: usize,
    strong: &[bool],
) -> bool {
    // 沿缺口两端朝外寻找延续像素，允许一步阶梯转折。
    [-1, 0, 1].into_iter().any(|side| {
        position(x + dx - dy * side, y + dy + dx * side, width, height).is_some_and(|i| strong[i])
    })
}

fn neighbours(i: usize, width: usize, height: usize) -> [Option<usize>; 4] {
    [
        (!i.is_multiple_of(width)).then(|| i - 1),
        (i % width + 1 < width).then_some(i + 1),
        (i >= width).then(|| i - width),
        (i + width < width * height).then_some(i + width),
    ]
}

fn exterior(width: usize, height: usize, barrier: &[bool]) -> Vec<bool> {
    let area = width * height;
    let mut outside = vec![false; area];
    let mut queue = Vec::new();
    let visit = |i: usize, outside: &mut [bool], queue: &mut Vec<usize>| {
        if !outside[i] && !barrier[i] {
            outside[i] = true;
            queue.push(i);
        }
    };
    for x in 0..width {
        visit(x, &mut outside, &mut queue);
        visit((height - 1) * width + x, &mut outside, &mut queue);
    }
    for y in 0..height {
        visit(y * width, &mut outside, &mut queue);
        visit(y * width + width - 1, &mut outside, &mut queue);
    }
    let mut head = 0;
    while head < queue.len() {
        let i = queue[head];
        head += 1;
        for next in neighbours(i, width, height).into_iter().flatten() {
            visit(next, &mut outside, &mut queue);
        }
    }
    outside
}

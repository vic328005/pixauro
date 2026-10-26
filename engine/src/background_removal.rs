//! 纯色背景抠图：边缘连通填充、内部空洞识别、过渡带去色污染和可选阴影保留。
//! 直接处理像素缓冲区，不依赖项目状态、命令或文件 IO。

use crate::ProjectError;

/// 抠图参数
#[derive(Clone, Debug)]
pub struct BackgroundRemovalOptions {
    /// 背景容差（RGB 欧氏距离），有限且非负，越大吃得越多
    pub tol: f32,
    /// 内部空洞的容差，有限且非负，0 表示不处理内部空洞
    pub hole_tol: f32,
    /// 内部空洞最小面积（像素）
    pub hole_min: usize,
    /// 保留脚下阴影（转成半透明黑）
    pub keep_shadow: bool,
    /// 保留阴影时，“纯背景”的容差，有限且非负
    pub shadow_core_tol: f32,
    /// 阴影最深处的不透明度，范围 0..=1
    pub shadow_max_alpha: f32,
    /// 描边过渡带宽度（像素），有限且非负，0 表示禁用
    pub edge: f32,
    /// alpha 只取 0 / 255（像素风用）
    pub hard: bool,
    /// 小于这个面积的孤立碎片会被清掉
    pub min_speck: usize,
}

impl Default for BackgroundRemovalOptions {
    fn default() -> Self {
        Self {
            tol: 40.0,
            hole_tol: 6.0,
            hole_min: 300,
            keep_shadow: false,
            shadow_core_tol: 12.0,
            shadow_max_alpha: 0.45,
            edge: 2.5,
            hard: false,
            min_speck: 20,
        }
    }
}

impl BackgroundRemovalOptions {
    fn validate(&self) -> Result<(), ProjectError> {
        for (name, value) in [
            ("背景容差", self.tol),
            ("内部空洞容差", self.hole_tol),
            ("阴影核心容差", self.shadow_core_tol),
            ("过渡带宽度", self.edge),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(invalid(&format!("抠图{name}必须为有限非负数")));
            }
        }
        if !self.shadow_max_alpha.is_finite() || !(0.0..=1.0).contains(&self.shadow_max_alpha) {
            return Err(invalid("抠图阴影不透明度必须在 0..=1 内"));
        }
        Ok(())
    }
}

fn invalid(message: &str) -> ProjectError {
    ProjectError::InvalidData(message.into())
}

/// 抠图结果
#[derive(Debug)]
pub struct BackgroundRemovalOutput {
    /// RGBA8，长度 width*height*4
    pub rgba: Vec<u8>,
    /// 估计出的背景色
    pub background: [f32; 3],
}

type Rgb = [f32; 3];

#[inline]
fn sub(a: Rgb, b: Rgb) -> Rgb {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn dot(a: Rgb, b: Rgb) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn mean3(a: Rgb) -> f32 {
    (a[0] + a[1] + a[2]) / 3.0
}

/// 抠掉纯色背景，返回同尺寸 RGBA8 和估计背景色（通道范围 0..=255）。
///
/// 适用于背景占据四边、角色有闭合深色描边的图片。内部空洞识别是启发式，
/// 可能移除与背景同色的封闭前景；可设置 `hole_tol = 0.0` 禁用。
/// 所有面积与距离参数都以输入图像像素为单位。此函数不修改输入或作品状态。
///
/// # 错误
/// 尺寸为零、像素数超过 u32 索引范围、缓冲区长度不匹配、通道数不为 3/4，
/// 或浮点参数不满足取值范围时，返回 `ProjectError::InvalidData`。
///
/// * `pixels`：RGB8 或 RGBA8（原有 alpha 会被忽略），行优先
/// * `channels`：3 或 4
pub fn remove_background(
    pixels: &[u8],
    width: usize,
    height: usize,
    channels: usize,
    opt: &BackgroundRemovalOptions,
) -> Result<BackgroundRemovalOutput, ProjectError> {
    if channels != 3 && channels != 4 {
        return Err(invalid("抠图输入必须为 RGB8 或 RGBA8"));
    }
    let n = width
        .checked_mul(height)
        .filter(|&n| n > 0 && n <= u32::MAX as usize)
        .filter(|&n| n.checked_mul(4).is_some())
        .ok_or_else(|| invalid("抠图尺寸必须非零，且像素数与缓冲区长度不能溢出"))?;
    if pixels.len() != n * channels {
        return Err(invalid("抠图像素缓冲区长度与尺寸、通道数不匹配"));
    }
    opt.validate()?;
    let (w, h) = (width, height);

    let rgb: Vec<Rgb> = pixels
        .chunks_exact(channels)
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
        .collect();

    let bgc = estimate_bg(&rgb, w, h);
    let b_norm = dot(bgc, bgc).sqrt();
    let b_mean = mean3(bgc);

    let diff: Vec<Rgb> = rgb.iter().map(|&c| sub(c, bgc)).collect();
    let d: Vec<f32> = diff.iter().map(|&v| dot(v, v).sqrt()).collect();
    let lum: Vec<f32> = rgb.iter().map(|&c| mean3(c)).collect();

    // ---- 1. 背景：与边缘连通、且颜色接近背景（或是“变暗的背景”= 阴影）----
    let loose_mask: Vec<bool> = (0..n)
        .map(|i| {
            let c = rgb[i];
            let ratio = lum[i] / b_mean.max(f32::EPSILON);
            let cos = dot(c, bgc) / (dot(c, c).sqrt() * b_norm + 1e-6);
            let shadow_like = cos > 0.9995 && ratio > 0.8 && ratio < 1.05;
            d[i] < opt.tol || shadow_like
        })
        .collect();
    let loose = flood_from_border(&loose_mask, w, h);

    let (mut bg, shadow) = if opt.keep_shadow {
        let core_mask: Vec<bool> = d.iter().map(|&v| v < opt.shadow_core_tol).collect();
        let bg = flood_from_border(&core_mask, w, h);
        let shadow: Vec<bool> = (0..n).map(|i| loose[i] && !bg[i]).collect();
        (bg, shadow)
    } else {
        (loose, vec![false; n])
    };

    // ---- 2. 被身体围住的背景小洞 ----
    if opt.hole_tol > 0.0 {
        let cand: Vec<bool> = (0..n)
            .map(|i| d[i] < opt.hole_tol && !bg[i] && !shadow[i])
            .collect();
        let (lab, count) = label(&cand, w, h);
        if count > 0 {
            // 洞需要贴着描边（附近有深色像素），避免把头部高光当成洞
            let dark: Vec<bool> = lum.iter().map(|&l| l < 80.0).collect();
            let near_dark = dilate_cross(&dark, w, h, 3);
            let mut size = vec![0usize; count + 1];
            let mut touch = vec![0usize; count + 1];
            for i in 0..n {
                let l = lab[i] as usize;
                if l > 0 {
                    size[l] += 1;
                    if near_dark[i] {
                        touch[l] += 1;
                    }
                }
            }
            let keep: Vec<bool> = (0..=count)
                .map(|l| {
                    l > 0
                        && size[l] >= opt.hole_min
                        && (touch[l] as f32 / size[l].max(1) as f32) > 0.05
                })
                .collect();
            for i in 0..n {
                if keep[lab[i] as usize] {
                    bg[i] = true;
                }
            }
        }
    }

    let fg: Vec<bool> = (0..n).map(|i| !bg[i] && !shadow[i]).collect();
    let mut alpha: Vec<f32> = fg.iter().map(|&f| if f { 1.0 } else { 0.0 }).collect();
    let mut out = rgb.clone();

    // ---- 3. 描边过渡带：反解 alpha + 去背景色污染 ----
    let not_fg: Vec<bool> = fg.iter().map(|&f| !f).collect();
    let (dist_to_bg, _) = edt_with_nearest(&not_fg, w, h);
    let band: Vec<bool> = (0..n).map(|i| fg[i] && dist_to_bg[i] <= opt.edge).collect();
    let core: Vec<bool> = (0..n).map(|i| fg[i] && dist_to_bg[i] > opt.edge).collect();
    if band.iter().any(|&b| b) && core.iter().any(|&c| c) {
        let (_, nearest) = edt_with_nearest(&core, w, h);
        for i in 0..n {
            if !band[i] {
                continue;
            }
            // 每个过渡像素最近的“实心前景色”
            let f = rgb[nearest[i] as usize];
            let fb = sub(f, bgc);
            let denom = dot(fb, fb);
            let a = if denom < 30.0 * 30.0 {
                1.0 // 前景本身接近背景色时不反解
            } else {
                (dot(diff[i], fb) / denom).clamp(0.0, 1.0)
            };
            alpha[i] = a;
            let safe = a.max(0.05);
            for c in 0..3 {
                out[i][c] = (bgc[c] + diff[i][c] / safe).clamp(0.0, 255.0);
            }
        }
    }

    // ---- 4. 阴影：变成半透明黑 ----
    if opt.keep_shadow && b_mean > 0.0 && shadow.iter().any(|&s| s) {
        let s_of = |i: usize| (1.0 - lum[i] / b_mean).clamp(0.0, 1.0);
        let smax = (0..n)
            .filter(|&i| shadow[i])
            .map(s_of)
            .fold(0.0f32, f32::max)
            .max(1e-6);
        for i in 0..n {
            if shadow[i] {
                out[i] = [0.0; 3];
                alpha[i] = (s_of(i) / smax * opt.shadow_max_alpha).clamp(0.0, opt.shadow_max_alpha);
            }
        }
    }

    if opt.hard {
        for a in alpha.iter_mut() {
            *a = if *a >= 0.5 { 1.0 } else { 0.0 };
        }
    }

    // ---- 5. 清掉孤立的小噪点 ----
    if opt.min_speck > 0 {
        let visible: Vec<bool> = alpha.iter().map(|&a| a > 0.0).collect();
        let (lab, count) = label(&visible, w, h);
        let mut size = vec![0usize; count + 1];
        for &l in &lab {
            size[l as usize] += 1;
        }
        for i in 0..n {
            let l = lab[i] as usize;
            if l > 0 && size[l] < opt.min_speck {
                alpha[i] = 0.0;
            }
        }
    }

    let mut rgba = vec![0u8; n * 4];
    for i in 0..n {
        let a8 = (alpha[i] * 255.0).round().clamp(0.0, 255.0) as u8;
        if a8 > 0 {
            for c in 0..3 {
                rgba[i * 4 + c] = out[i][c].round().clamp(0.0, 255.0) as u8;
            }
        }
        rgba[i * 4 + 3] = a8;
    }

    Ok(BackgroundRemovalOutput {
        rgba,
        background: bgc,
    })
}

/// 四条边像素的逐通道中位数
fn estimate_bg(rgb: &[Rgb], w: usize, h: usize) -> Rgb {
    let mut border: Vec<Rgb> = Vec::with_capacity(2 * (w + h));
    border.extend_from_slice(&rgb[0..w]);
    border.extend_from_slice(&rgb[(h - 1) * w..h * w]);
    for y in 0..h {
        border.push(rgb[y * w]);
        border.push(rgb[y * w + w - 1]);
    }
    let mut res = [0.0f32; 3];
    for (c, r) in res.iter_mut().enumerate() {
        let mut v: Vec<f32> = border.iter().map(|p| p[c]).collect();
        v.sort_by(|a, b| a.total_cmp(b));
        let m = v.len();
        *r = if !m.is_multiple_of(2) {
            v[m / 2]
        } else {
            (v[m / 2 - 1] + v[m / 2]) * 0.5
        };
    }
    res
}

/// 4 邻域连通
#[inline]
fn neighbors4(i: usize, w: usize, h: usize) -> impl Iterator<Item = usize> {
    let (x, y) = (i % w, i / w);
    let mut nb = [usize::MAX; 4];
    if x > 0 {
        nb[0] = i - 1;
    }
    if x + 1 < w {
        nb[1] = i + 1;
    }
    if y > 0 {
        nb[2] = i - w;
    }
    if y + 1 < h {
        nb[3] = i + w;
    }
    nb.into_iter().filter(|&j| j != usize::MAX)
}

/// mask 中与图片边缘连通的部分
fn flood_from_border(mask: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut out = vec![false; w * h];
    let mut stack = Vec::new();
    let seed = |i: usize, out: &mut Vec<bool>, stack: &mut Vec<usize>| {
        if mask[i] && !out[i] {
            out[i] = true;
            stack.push(i);
        }
    };
    for x in 0..w {
        seed(x, &mut out, &mut stack);
        seed((h - 1) * w + x, &mut out, &mut stack);
    }
    for y in 0..h {
        seed(y * w, &mut out, &mut stack);
        seed(y * w + w - 1, &mut out, &mut stack);
    }
    while let Some(i) = stack.pop() {
        for j in neighbors4(i, w, h) {
            if mask[j] && !out[j] {
                out[j] = true;
                stack.push(j);
            }
        }
    }
    out
}

/// 连通域标记（4 邻域）。返回 (标签图, 连通域数)，标签从 1 开始，0 表示不在 mask 内
fn label(mask: &[bool], w: usize, h: usize) -> (Vec<u32>, usize) {
    let mut lab = vec![0u32; w * h];
    let mut count = 0u32;
    let mut stack = Vec::new();
    for s in 0..w * h {
        if !mask[s] || lab[s] != 0 {
            continue;
        }
        count += 1;
        lab[s] = count;
        stack.push(s);
        while let Some(i) = stack.pop() {
            for j in neighbors4(i, w, h) {
                if mask[j] && lab[j] == 0 {
                    lab[j] = count;
                    stack.push(j);
                }
            }
        }
    }
    (lab, count as usize)
}

/// 十字结构元素膨胀 `iters` 次
fn dilate_cross(mask: &[bool], w: usize, h: usize, iters: usize) -> Vec<bool> {
    let mut cur = mask.to_vec();
    for _ in 0..iters {
        let mut next = cur.clone();
        for i in 0..w * h {
            if !cur[i] {
                next[i] = neighbors4(i, w, h).any(|j| cur[j]);
            }
        }
        cur = next;
    }
    cur
}

/// 精确欧氏距离变换（Felzenszwalb–Huttenlocher），同时给出最近特征像素的下标。
/// 返回每个像素到最近 `feature == true` 像素的距离；没有特征像素时为 +inf / u32::MAX。
fn edt_with_nearest(feature: &[bool], w: usize, h: usize) -> (Vec<f32>, Vec<u32>) {
    const INF: f64 = f64::INFINITY;
    let n = w * h;

    // 列方向：到本列最近特征像素的平方距离 + 其 y 坐标
    let mut g = vec![INF; n];
    let mut gy = vec![0u32; n];
    for x in 0..w {
        let mut last: Option<usize> = None;
        for y in 0..h {
            let i = y * w + x;
            if feature[i] {
                last = Some(y);
            }
            if let Some(l) = last {
                let dy = (y - l) as f64;
                g[i] = dy * dy;
                gy[i] = l as u32;
            }
        }
        last = None;
        for y in (0..h).rev() {
            let i = y * w + x;
            if feature[i] {
                last = Some(y);
            }
            if let Some(l) = last {
                let dy = (l - y) as f64;
                if dy * dy < g[i] {
                    g[i] = dy * dy;
                    gy[i] = l as u32;
                }
            }
        }
    }

    // 行方向：抛物线下包络
    let mut dist = vec![f32::INFINITY; n];
    let mut nearest = vec![u32::MAX; n];
    let mut f = vec![0f64; w];
    let mut sites: Vec<usize> = Vec::with_capacity(w);
    let mut v = vec![0usize; w];
    let mut z = vec![0f64; w + 1];
    for y in 0..h {
        sites.clear();
        for x in 0..w {
            f[x] = g[y * w + x];
            if f[x].is_finite() {
                sites.push(x);
            }
        }
        if sites.is_empty() {
            continue;
        }
        let sq = |a: usize| (a as f64).powi(2);
        let mut k = 0usize;
        v[0] = sites[0];
        z[0] = -INF;
        z[1] = INF;
        for &q in &sites[1..] {
            let mut s;
            loop {
                let p = v[k];
                s = ((f[q] + sq(q)) - (f[p] + sq(p))) / (2.0 * (q - p) as f64);
                if s <= z[k] && k > 0 {
                    k -= 1;
                } else {
                    break;
                }
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = INF;
        }
        k = 0;
        for x in 0..w {
            while z[k + 1] < x as f64 {
                k += 1;
            }
            let p = v[k];
            let dx = x as f64 - p as f64;
            let i = y * w + x;
            dist[i] = (dx * dx + f[p]).sqrt() as f32;
            nearest[i] = gy[y * w + p] * w as u32 + p as u32;
        }
    }
    (dist, nearest)
}

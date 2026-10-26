//! PNG 解码、透明度预处理与迭代像素抽象。求解不持有 Engine 或桌面锁。
use crate::{Color, EditError, Engine, FrameId, LayerId, ProjectError};
use pixelart_abstraction::{Options, Pixelator, RgbImage, StopReason};
use serde::{Deserialize, Serialize};
use std::{fs::File, io::BufReader, ops::ControlFlow, path::Path};

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DECODE_BYTES: usize = 128 * 1024 * 1024;
const MAX_SOURCE_PIXELS: u64 = 32 * 1024 * 1024;
const MAX_GRID_PIXELS: u32 = 512 * 512;
// 原图抠图持有多张距离场与像素缓冲区，限制单次工作内存。
const MAX_BACKGROUND_PIXELS: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct PngInfo {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct PngImportOptions {
    pub pixel_size: u16,
    pub colors: u16,
    pub working_size: u16,
    pub saturation: f64,
    pub compactness: f64,
    pub smoothing: f64,
    pub color_sigma: f64,
    pub max_iterations: u16,
}

impl Default for PngImportOptions {
    fn default() -> Self {
        Self {
            pixel_size: 64,
            colors: 16,
            working_size: 1024,
            saturation: 1.1,
            compactness: 45.0,
            smoothing: 0.4,
            color_sigma: 3.0,
            max_iterations: 500,
        }
    }
}

impl PngImportOptions {
    pub(crate) fn validate(self) -> Result<(), ProjectError> {
        if !(1..=1024).contains(&self.pixel_size)
            || !(1..=256).contains(&self.colors)
            || !(128..=2048).contains(&self.working_size)
            || !(1..=2000).contains(&self.max_iterations)
            || !self.saturation.is_finite()
            || !(0.0..=2.0).contains(&self.saturation)
            || !self.compactness.is_finite()
            || !(1.0..=100.0).contains(&self.compactness)
            || !self.smoothing.is_finite()
            || !(0.0..=1.0).contains(&self.smoothing)
            || !self.color_sigma.is_finite()
            || !(0.1..=20.0).contains(&self.color_sigma)
        {
            return Err(invalid("导入参数超出允许范围"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum PngImportStage {
    Decoding,
    Preparing,
    Optimizing,
    Masking,
    Placing,
}

#[derive(Clone, Copy, Debug)]
pub struct PngImportProgress {
    pub stage: PngImportStage,
    pub completed: u32,
    pub total: u32,
    pub colors: u16,
}

/// 仅持有最终索引化之前的 RGBA，不保存求解会话或源文件路径。
pub struct PreparedPng {
    name: String,
    canvas_size: (u16, u16),
    pixels: Vec<((i32, i32), Color)>,
    foreground: Option<Vec<u8>>,
    pub iterations: u32,
    pub converged: bool,
}

fn invalid(message: &str) -> ProjectError {
    ProjectError::InvalidData(message.into())
}
fn decode_error(error: png::DecodingError) -> ProjectError {
    invalid(&format!("无法读取 PNG：{error}"))
}
fn reader(path: &Path) -> Result<png::Reader<BufReader<File>>, ProjectError> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(invalid("请导入不超过 64 MiB 的 PNG 文件"));
    }
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_limits(png::Limits {
        bytes: MAX_DECODE_BYTES,
    });
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let reader = decoder.read_info().map_err(decode_error)?;
    let info = reader.info();
    if info.animation_control.is_some() {
        return Err(invalid("暂不支持 APNG，请导入静态 PNG 图片"));
    }
    if u64::from(info.width) * u64::from(info.height) > MAX_SOURCE_PIXELS {
        return Err(invalid("PNG 超过 3200 万像素，请先缩小图片"));
    }
    Ok(reader)
}

pub fn inspect_png(path: impl AsRef<Path>) -> Result<PngInfo, ProjectError> {
    let reader = reader(path.as_ref())?;
    Ok(PngInfo {
        width: reader.info().width,
        height: reader.info().height,
    })
}

/// 等比 contain；仅对最后的像素边界取整。
fn fit(width: u32, height: u32, bounds: (u32, u32)) -> (u32, u32) {
    let (w, h, bw, bh) = (
        u64::from(width),
        u64::from(height),
        u64::from(bounds.0),
        u64::from(bounds.1),
    );
    if bw * h <= bh * w {
        (bw as u32, ((h * bw + w / 2) / w).max(1) as u32)
    } else {
        (((w * bh + h / 2) / h).max(1) as u32, bh as u32)
    }
}

/// 重计算和进度回调都在调用者的工作线程上执行。取消返回 None，不修改作品。
pub fn prepare_png_import(
    path: impl AsRef<Path>,
    canvas_size: (u16, u16),
    options: PngImportOptions,
    mut progress: impl FnMut(PngImportProgress) -> ControlFlow<()>,
) -> Result<Option<PreparedPng>, EditError> {
    options.validate()?;
    if canvas_size.0 == 0 || canvas_size.1 == 0 {
        return Err(invalid("画布尺寸无效").into());
    }
    if progress(PngImportProgress {
        stage: PngImportStage::Decoding,
        completed: 0,
        total: 1,
        colors: 0,
    })
    .is_break()
    {
        return Ok(None);
    }
    let path = path.as_ref();
    let decoded = decode(path)?;
    prepare_region(
        &decoded,
        (0, 0, decoded.info.width, decoded.info.height),
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("导入图片"),
        canvas_size,
        options,
        progress,
    )
}

/// 区域视图直接读取原图坐标，不复制整张图，也不在裁切前缩小原图。
fn prepare_region(
    decoded: &DecodedPng,
    region: (u32, u32, u32, u32),
    name: &str,
    canvas_size: (u16, u16),
    options: PngImportOptions,
    mut progress: impl FnMut(PngImportProgress) -> ControlFlow<()>,
) -> Result<Option<PreparedPng>, EditError> {
    let (left, top, width, height) = region;
    let mut report = |stage, completed, total, colors| {
        progress(PngImportProgress {
            stage,
            completed,
            total,
            colors,
        })
        .is_break()
    };
    let final_size = fit(
        width,
        height,
        (u32::from(canvas_size.0), u32::from(canvas_size.1)),
    );
    let working_size = fit(
        width,
        height,
        (
            width.min(u32::from(options.working_size)),
            height.min(u32::from(options.working_size)),
        ),
    );
    let grid = fit(
        width,
        height,
        (
            final_size
                .0
                .min(u32::from(options.pixel_size))
                .min(working_size.0),
            final_size
                .1
                .min(u32::from(options.pixel_size))
                .min(working_size.1),
        ),
    );
    if grid.0 * grid.1 > MAX_GRID_PIXELS {
        return Err(invalid("单帧像素网格最多 262144 格，请减小像素尺寸").into());
    }
    let sample = |x, y| decoded.sample(left + x, top + y);
    // 输入预缩小采用面积覆盖与预乘 alpha 统计，不把隐藏 RGB 混进边缘。
    let mut rgba = Vec::with_capacity((working_size.0 * working_size.1) as usize);
    let sx = f64::from(width) / f64::from(working_size.0);
    let sy = f64::from(height) / f64::from(working_size.1);
    for y in 0..working_size.1 {
        if report(PngImportStage::Preparing, y, working_size.1, 0) {
            return Ok(None);
        }
        let (y0, y1) = (f64::from(y) * sy, f64::from(y + 1) * sy);
        for x in 0..working_size.0 {
            let (x0, x1) = (f64::from(x) * sx, f64::from(x + 1) * sx);
            let mut sum = [0.0; 4];
            for iy in y0.floor() as u32..(y1.ceil() as u32).min(height) {
                let wy = (y1.min(f64::from(iy + 1)) - y0.max(f64::from(iy))).max(0.0);
                for ix in x0.floor() as u32..(x1.ceil() as u32).min(width) {
                    let area = wy * (x1.min(f64::from(ix + 1)) - x0.max(f64::from(ix))).max(0.0);
                    let p = sample(ix, iy);
                    let weight = area * f64::from(p[3]);
                    for c in 0..3 {
                        sum[c] += f64::from(p[c]) * weight;
                    }
                    sum[3] += weight;
                }
            }
            rgba.push(if sum[3] == 0.0 {
                [0; 4]
            } else {
                [
                    (sum[0] / sum[3]).round() as u8,
                    (sum[1] / sum[3]).round() as u8,
                    (sum[2] / sum[3]).round() as u8,
                    (sum[3] / (sx * sy)).round().clamp(0.0, 255.0) as u8,
                ]
            });
        }
    }
    let name: String = name.chars().filter(|c| !c.is_control()).take(128).collect();
    let mut prepared = PreparedPng {
        name: if name.trim().is_empty() {
            "导入图片".into()
        } else {
            name.trim().into()
        },
        canvas_size,
        pixels: Vec::new(),
        foreground: None,
        iterations: 0,
        converged: true,
    };
    if rgba.iter().all(|p| p[3] == 0) {
        prepared.foreground = Some(vec![
            1;
            usize::from(canvas_size.0) * usize::from(canvas_size.1)
        ]);
        return Ok(Some(prepared));
    }
    if report(
        PngImportStage::Optimizing,
        0,
        u32::from(options.max_iterations),
        0,
    ) {
        return Ok(None);
    }
    let input = RgbImage::from_rgba(working_size.0 as usize, working_size.1 as usize, &rgba)
        .map_err(|error| invalid(&error.to_string()))?;
    drop(rgba);
    let mut config = Options::new(
        grid.0 as usize,
        grid.1 as usize,
        usize::from(options.colors),
    );
    config.saturation = options.saturation;
    config.compactness = options.compactness;
    config.position_smoothing = options.smoothing;
    config.bilateral.color_sigma = options.color_sigma;
    config.max_iterations = usize::from(options.max_iterations);
    let mut solver = Pixelator::new(&input, config).map_err(|error| invalid(&error.to_string()))?;
    drop(input);
    let mut cancelled = false;
    let result = solver.run_with_progress(|p| {
        cancelled = report(
            PngImportStage::Optimizing,
            p.iteration as u32,
            u32::from(options.max_iterations),
            p.palette_colors as u16,
        );
        if cancelled {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    if cancelled || result.statistics.stop_reason == StopReason::Cancelled {
        return Ok(None);
    }
    drop(solver);
    if report(PngImportStage::Masking, 0, 1, result.palette.len() as u16) {
        return Ok(None);
    }
    // 去背只依据用户最终看到的像素颜色与轮廓，不继承原图前景占比。
    let grid_rgba: Vec<_> = result
        .indices
        .iter()
        .zip(&result.alpha)
        .map(|(&index, &alpha)| {
            let [r, g, b] = result.palette[usize::from(index)];
            [r, g, b, alpha]
        })
        .collect();
    let grid_foreground = super::foreground::estimate(grid.0 as usize, grid.1 as usize, &grid_rgba);
    drop(grid_rgba);
    if report(PngImportStage::Masking, 1, 1, result.palette.len() as u16) {
        return Ok(None);
    }
    if grid_foreground.is_some() {
        prepared.foreground = Some(vec![
            0;
            usize::from(canvas_size.0) * usize::from(canvas_size.1)
        ]);
    }
    prepared.iterations = result.statistics.iterations as u32;
    prepared.converged = result.statistics.stop_reason == StopReason::Converged;
    let left = (u32::from(canvas_size.0) - final_size.0) / 2;
    let top = (u32::from(canvas_size.1) - final_size.1) / 2;
    // 仅把算法生成的格子展开到放置尺寸，不再对原图做最近邻像素化。
    for y in 0..final_size.1 {
        if report(
            PngImportStage::Placing,
            y,
            final_size.1,
            result.palette.len() as u16,
        ) {
            return Ok(None);
        }
        let gy = ((2 * y + 1) * grid.1 / (2 * final_size.1)) as usize;
        for x in 0..final_size.0 {
            let gx = ((2 * x + 1) * grid.0 / (2 * final_size.0)) as usize;
            let i = gy * grid.0 as usize + gx;
            if let (Some(mask), Some(output)) = (&grid_foreground, &mut prepared.foreground) {
                output[((top + y) * u32::from(canvas_size.0) + left + x) as usize] = mask[i];
            }
            let alpha = result.alpha[i];
            if alpha == 0 {
                continue;
            }
            let [r, g, b] = result.palette[usize::from(result.indices[i])];
            prepared.pixels.push((
                ((left + x) as i32, (top + y) as i32),
                Color::rgba(r, g, b, alpha),
            ));
        }
    }
    Ok(Some(prepared))
}

impl Engine {
    /// 提交已完成的抽象结果，一次历史操作包含图层、像素与外部颜色。
    pub fn apply_png_import(
        &mut self,
        prepared: PreparedPng,
        frame: FrameId,
        above: LayerId,
    ) -> Result<LayerId, EditError> {
        let canvas = self.project().canvas();
        if prepared.canvas_size != (canvas.width(), canvas.height()) {
            return Err(invalid("画布已变化，请重新导入").into());
        }
        let position = canvas
            .layers()
            .iter()
            .position(|layer| layer.id() == above)
            .ok_or(crate::DrawError::InvalidLayerId(above))?
            + 1;
        self.execute(crate::commands::ImportImageCommand::new(
            frame,
            above,
            prepared.name,
            prepared.pixels,
        ))?;
        Ok(self.project().canvas().layers()[position].id())
    }
}

struct DecodedPng {
    info: png::OutputInfo,
    bytes: Vec<u8>,
}
impl DecodedPng {
    fn remove_background(&mut self, edge: f32) -> Result<(), ProjectError> {
        if !edge.is_finite() || edge < 0.0 {
            return Err(invalid("原图去背边缘处理宽度必须为有限非负数"));
        }
        let (width, height) = (self.info.width as usize, self.info.height as usize);
        if u64::from(self.info.width) * u64::from(self.info.height) > MAX_BACKGROUND_PIXELS {
            return Err(invalid(
                "原图背景剔除最多支持 4194304 像素，请缩小原图或使用后续去背",
            ));
        }
        let mut rgba = Vec::with_capacity(width * height * 4);
        for y in 0..self.info.height {
            for x in 0..self.info.width {
                rgba.extend_from_slice(&self.sample(x, y));
            }
        }
        let mut output = crate::remove_background(
            &rgba,
            width,
            height,
            4,
            &crate::BackgroundRemovalOptions {
                edge,
                ..crate::BackgroundRemovalOptions::default()
            },
        )?;
        // 独立算法重建 alpha；导入预处理叠乘原透明度，避免复活透明像素。
        for (pixel, original) in output
            .rgba
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(rgba.as_chunks::<4>().0)
        {
            pixel[3] = ((u16::from(pixel[3]) * u16::from(original[3]) + 127) / 255) as u8;
            if pixel[3] == 0 {
                pixel.fill(0);
            }
        }
        self.bytes = output.rgba;
        self.info.color_type = png::ColorType::Rgba;
        self.info.bit_depth = png::BitDepth::Eight;
        self.info.line_size = width * 4;
        Ok(())
    }

    fn sample(&self, x: u32, y: u32) -> [u8; 4] {
        let offset = y as usize * self.info.line_size + x as usize * self.info.color_type.samples();
        let p = &self.bytes[offset..];
        match self.info.color_type {
            png::ColorType::Rgba => [p[0], p[1], p[2], p[3]],
            png::ColorType::Rgb => [p[0], p[1], p[2], 255],
            png::ColorType::Grayscale => [p[0], p[0], p[0], 255],
            png::ColorType::GrayscaleAlpha => [p[0], p[0], p[0], p[1]],
            png::ColorType::Indexed => unreachable!("normalize_to_color8 展开索引色"),
        }
    }
}
fn decode(path: &Path) -> Result<DecodedPng, ProjectError> {
    let mut reader = reader(path)?;
    let size = reader
        .output_buffer_size()
        .filter(|size| *size <= MAX_DECODE_BYTES)
        .ok_or_else(|| invalid("PNG 解码后超过 128 MiB"))?;
    let mut bytes = vec![0; size];
    let info = reader.next_frame(&mut bytes).map_err(decode_error)?;
    reader.finish().map_err(decode_error)?;
    Ok(DecodedPng { info, bytes })
}

/// 导入工作区的临时位图，不写入正式作品。
#[derive(Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PngRaster {
    pub source_width: u32,
    pub source_height: u32,
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    pub foreground: Option<Vec<u8>>,
    pub iterations: u32,
    pub converged: bool,
}

/// 可在缩小预览前对完整原图去背；源文件不变。
pub fn png_source_preview(
    path: impl AsRef<Path>,
    remove_source_background: bool,
    source_background_edge: f32,
) -> Result<PngRaster, ProjectError> {
    let mut decoded = decode(path.as_ref())?;
    if remove_source_background {
        decoded.remove_background(source_background_edge)?;
    }
    let (sw, sh) = (decoded.info.width, decoded.info.height);
    let (w, h) = fit(sw, sh, (sw.min(1024), sh.min(1024)));
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&decoded.sample(x * sw / w, y * sh / h));
        }
    }
    Ok(PngRaster {
        source_width: sw,
        source_height: sh,
        width: w as u16,
        height: h as u16,
        rgba,
        foreground: None,
        iterations: 0,
        converged: true,
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpriteSlice {
    pub id: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpriteFramePreview {
    pub id: String,
    pub raster: PngRaster,
}

#[derive(Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpritePreview {
    pub source_width: u32,
    pub source_height: u32,
    pub frames: Vec<SpriteFramePreview>,
}

pub struct SpriteImportProgress {
    pub completed: u32,
    pub total: u32,
    pub active: u32,
}

/// 原图仅解码一次；可先对整张原图去背，再对每个区域独立预处理与求解。
/// 使用与原图预览相同的全分辨率去背流程，按输入顺序返回。
/// 进度回调只在调用线程执行，取消或任意一帧失败均丢弃整批临时结果。
pub fn prepare_sprite_preview(
    path: impl AsRef<Path>,
    slices: &[SpriteSlice],
    canvas_size: (u16, u16),
    options: PngImportOptions,
    remove_source_background: bool,
    source_background_edge: f32,
    progress: impl FnMut(SpriteImportProgress) -> ControlFlow<()>,
) -> Result<Option<SpritePreview>, EditError> {
    prepare_sprite_rasters(
        path,
        slices,
        canvas_size,
        Some(options),
        remove_source_background,
        source_background_edge,
        progress,
    )
}

/// 资源流水线共用入口；options=None 时保留裁切像素，不缩放、不量化。
pub fn prepare_sprite_rasters(
    path: impl AsRef<Path>,
    slices: &[SpriteSlice],
    canvas_size: (u16, u16),
    options: Option<PngImportOptions>,
    remove_source_background: bool,
    source_background_edge: f32,
    mut progress: impl FnMut(SpriteImportProgress) -> ControlFlow<()>,
) -> Result<Option<SpritePreview>, EditError> {
    use std::{
        collections::HashSet,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };
    if let Some(options) = options {
        options.validate()?;
    }
    if slices.is_empty() || slices.len() > 256 || canvas_size.0 == 0 || canvas_size.1 == 0 {
        return Err(invalid("请选择 1 到 256 个切片，并使用有效画布尺寸").into());
    }
    let total = slices.len() as u32;
    if progress(SpriteImportProgress {
        completed: 0,
        total,
        active: 0,
    })
    .is_break()
    {
        return Ok(None);
    }
    let mut decoded = decode(path.as_ref())?;
    let (source_width, source_height) = (decoded.info.width, decoded.info.height);
    let mut ids = HashSet::new();
    let mut output_pixels = 0_u64;
    let mut sizes = Vec::with_capacity(slices.len());
    for slice in slices {
        if slice.id.is_empty()
            || slice.id.len() > 128
            || !ids.insert(&slice.id)
            || slice.width == 0
            || slice.height == 0
            || slice
                .x
                .checked_add(slice.width)
                .is_none_or(|x| x > source_width)
            || slice
                .y
                .checked_add(slice.height)
                .is_none_or(|y| y > source_height)
        {
            return Err(invalid("切片 ID 或原图裁切范围无效，请重新选择切片").into());
        }
        let size = if let Some(options) = options {
            let side = u32::from(options.pixel_size.min(options.working_size));
            let size = fit(
                slice.width,
                slice.height,
                (
                    slice.width.min(side).min(u32::from(canvas_size.0)),
                    slice.height.min(side).min(u32::from(canvas_size.1)),
                ),
            );
            if size.0 * size.1 > MAX_GRID_PIXELS {
                return Err(invalid("单帧像素网格最多 262144 格，请减小像素尺寸").into());
            }
            size
        } else {
            if slice.width > 4096 || slice.height > 4096 {
                return Err(invalid("不做像素化时，单帧宽高最多 4096").into());
            }
            (slice.width, slice.height)
        };
        output_pixels += u64::from(size.0) * u64::from(size.1);
        sizes.push((size.0 as u16, size.1 as u16));
    }
    if output_pixels > 4096 * 4096 {
        return Err(invalid("所有帧的输出像素合计最多 4096²，请减少帧数或像素尺寸").into());
    }
    if progress(SpriteImportProgress {
        completed: 0,
        total,
        active: 0,
    })
    .is_break()
    {
        return Ok(None);
    }
    if remove_source_background {
        decoded.remove_background(source_background_edge)?;
        if progress(SpriteImportProgress {
            completed: 0,
            total,
            active: 0,
        })
        .is_break()
        {
            return Ok(None);
        }
    }
    enum Event {
        Started,
        Finished(usize, Result<Option<PngRaster>, EditError>),
    }
    let worker_count = thread::available_parallelism()
        .map_or(1, usize::from)
        .min(4)
        .min(slices.len());
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let mut results: Vec<Option<PngRaster>> = (0..slices.len()).map(|_| None).collect();
    let mut completed = 0;
    let mut active = 0;
    let mut cancelled = false;
    let mut error = None;
    thread::scope(|scope| {
        let (sender, receiver) = mpsc::channel();
        let mut workers = Vec::new();
        for _ in 0..worker_count {
            let (sender, decoded, sizes, next, stop) =
                (sender.clone(), &decoded, &sizes, &next, &stop);
            workers.push(scope.spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(slice) = slices.get(index) else {
                        break;
                    };
                    if sender.send(Event::Started).is_err() {
                        break;
                    }
                    let result = if let Some(options) = options {
                        prepare_region(
                            decoded,
                            (slice.x, slice.y, slice.width, slice.height),
                            &slice.id,
                            sizes[index],
                            options,
                            |_| {
                                if stop.load(Ordering::Relaxed) {
                                    ControlFlow::Break(())
                                } else {
                                    ControlFlow::Continue(())
                                }
                            },
                        )
                        .map(|prepared| {
                            prepared.map(|prepared| {
                                let (w, h) = sizes[index];
                                let mut rgba = vec![0; usize::from(w) * usize::from(h) * 4];
                                for ((x, y), c) in prepared.pixels {
                                    let i = (y as usize * usize::from(w) + x as usize) * 4;
                                    rgba[i..i + 4]
                                        .copy_from_slice(&[c.red, c.green, c.blue, c.alpha]);
                                }
                                PngRaster {
                                    source_width: slice.width,
                                    source_height: slice.height,
                                    width: w,
                                    height: h,
                                    rgba,
                                    foreground: prepared.foreground,
                                    iterations: prepared.iterations,
                                    converged: prepared.converged,
                                }
                            })
                        })
                    } else {
                        crop_region(decoded, slice, || stop.load(Ordering::Relaxed))
                    };
                    if result.is_err() {
                        stop.store(true, Ordering::Relaxed);
                    }
                    if sender.send(Event::Finished(index, result)).is_err() {
                        break;
                    }
                }
            }));
        }
        drop(sender);
        loop {
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(Event::Started) => active += 1,
                Ok(Event::Finished(index, result)) => {
                    active -= 1;
                    match result {
                        Ok(Some(raster)) => {
                            results[index] = Some(raster);
                            completed += 1;
                        }
                        Ok(None) => {}
                        Err(reason) => {
                            if error.is_none() {
                                error = Some(
                                    invalid(&format!("第 {} 帧像素化失败：{reason}", index + 1))
                                        .into(),
                                );
                            }
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if !cancelled
                && progress(SpriteImportProgress {
                    completed,
                    total,
                    active,
                })
                .is_break()
            {
                cancelled = true;
                stop.store(true, Ordering::Relaxed);
            }
        }
        for worker in workers {
            if worker.join().is_err() && error.is_none() {
                error = Some(invalid("像素化工作线程异常结束，请减小工作输入尺寸后重试").into());
            }
        }
    });
    if let Some(error) = error {
        return Err(error);
    }
    if cancelled {
        return Ok(None);
    }
    let frames = slices
        .iter()
        .zip(results)
        .map(|(slice, raster)| {
            raster
                .map(|raster| SpriteFramePreview {
                    id: slice.id.clone(),
                    raster,
                })
                .ok_or_else(|| EditError::from(invalid("像素化未完成，请重新生成预览")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(SpritePreview {
        source_width,
        source_height,
        frames,
    }))
}

fn crop_region(
    decoded: &DecodedPng,
    slice: &SpriteSlice,
    cancelled: impl Fn() -> bool,
) -> Result<Option<PngRaster>, EditError> {
    let mut pixels = Vec::with_capacity(slice.width as usize * slice.height as usize);
    for y in 0..slice.height {
        if cancelled() {
            return Ok(None);
        }
        for x in 0..slice.width {
            pixels.push(decoded.sample(slice.x + x, slice.y + y));
        }
    }
    let foreground =
        super::foreground::estimate(slice.width as usize, slice.height as usize, &pixels);
    Ok(Some(PngRaster {
        source_width: slice.width,
        source_height: slice.height,
        width: slice.width as u16,
        height: slice.height as u16,
        rgba: pixels.into_iter().flatten().collect(),
        foreground,
        iterations: 0,
        converged: true,
    }))
}

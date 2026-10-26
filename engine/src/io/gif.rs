//! GIF 只用于动画预览：透明度二值化、最近邻整数放大、循环播放。
use std::{collections::BTreeMap, path::Path};

use super::atomic_file::{WriteMode, write_atomic};
use crate::{Animation, AnimationId, Color, EditError, Engine, ProjectError};

#[derive(Clone, Copy, Debug)]
pub struct GifExportOptions {
    /// 最近邻整数倍率，1～16；放大后的宽高最多 4096。
    pub scale: u16,
    /// 小于该值的 alpha 变为全透明，其余变为全不透明，1～255。
    pub alpha_threshold: u8,
}
impl Default for GifExportOptions {
    fn default() -> Self {
        Self {
            scale: 1,
            alpha_threshold: 128,
        }
    }
}
impl GifExportOptions {
    pub fn validate(self, width: u16, height: u16, frames: usize) -> Result<(), ProjectError> {
        let w = u64::from(width) * u64::from(self.scale);
        let h = u64::from(height) * u64::from(self.scale);
        if !(1..=16).contains(&self.scale)
            || self.alpha_threshold == 0
            || w == 0
            || h == 0
            || w > 4096
            || h > 4096
            || frames == 0
            || frames > 4096
            || w * h * frames as u64 > 256 * 1024 * 1024
        {
            return Err(ProjectError::InvalidData("GIF 倍率须为 1～16，alpha 阈值须为 1～255，输出宽高最多 4096、总帧像素最多 256 Mi，帧数须为 1～4096".into()));
        }
        Ok(())
    }
}

impl Engine {
    /// 导出指定动画的可见合成帧，不修改作品或历史。
    pub fn export_animation_gif(
        &self,
        animation: AnimationId,
        path: impl AsRef<Path>,
        options: GifExportOptions,
    ) -> Result<(), EditError> {
        let canvas = self.project().canvas();
        let animation = &canvas.animations()[canvas.animation(animation)?];
        write_animation_gif(
            path.as_ref(),
            (canvas.width(), canvas.height()),
            animation.frames.len(),
            animation.fps,
            options,
            |index, x, y| {
                self.project().composite_frame_at(
                    animation.frames[index].frame,
                    y * usize::from(canvas.width()) + x,
                )
            },
        )?;
        Ok(())
    }
}

fn palette_frame(
    pixels: &[[u8; 4]],
    width: u16,
    height: u16,
    options: GifExportOptions,
) -> gif::Frame<'static> {
    // 每帧都预留透明索引 0，包括全不透明帧；背景清除始终恢复透明画布。
    let mut colors = BTreeMap::new();
    for p in pixels.iter().filter(|p| p[3] >= options.alpha_threshold) {
        let next = colors.len() + 1;
        colors.entry([p[0], p[1], p[2]]).or_insert(next);
        if colors.len() > 255 {
            break;
        }
    }
    let mut palette = vec![0; 3];
    let indices: Vec<u8> = if colors.len() <= 255 {
        palette.resize((colors.len() + 1) * 3, 0);
        for (color, &index) in &colors {
            palette[index * 3..index * 3 + 3].copy_from_slice(color);
        }
        pixels
            .iter()
            .map(|p| {
                if p[3] < options.alpha_threshold {
                    0
                } else {
                    colors[&[p[0], p[1], p[2]]] as u8
                }
            })
            .collect()
    } else {
        // 仅颜色超过 GIF 容量时量化，隐藏 RGB 和半透明 alpha 不参与调色。
        let opaque: Vec<u8> = pixels
            .iter()
            .filter(|p| p[3] >= options.alpha_threshold)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect();
        let quantizer = color_quant::NeuQuant::new(10, 255, &opaque);
        palette.extend(quantizer.color_map_rgb());
        pixels
            .iter()
            .map(|p| {
                if p[3] < options.alpha_threshold {
                    0
                } else {
                    (quantizer.index_of(&[p[0], p[1], p[2], 255]) + 1) as u8
                }
            })
            .collect()
    };
    let scale = usize::from(options.scale);
    let w = usize::from(width);
    let mut scaled = Vec::with_capacity(indices.len() * scale * scale);
    for y in 0..usize::from(height) * scale {
        for x in 0..w * scale {
            scaled.push(indices[y / scale * w + x / scale]);
        }
    }
    gif::Frame::from_palette_pixels(
        width * options.scale,
        height * options.scale,
        scaled,
        palette,
        Some(0),
    )
}

pub(crate) fn write_animation_gif(
    path: &Path,
    size: (u16, u16),
    frames: usize,
    fps: f64,
    options: GifExportOptions,
    color_at: impl Fn(usize, usize, usize) -> Color,
) -> Result<(), ProjectError> {
    let (w, h) = size;
    options.validate(w, h, frames)?;
    if !Animation::valid_fps(fps) {
        return Err(ProjectError::InvalidData("GIF 帧率必须为 0.2～100".into()));
    }
    write_atomic(path, WriteMode::Replace, |file| {
        let mut encoder = gif::Encoder::new(
            file,
            w * options.scale,
            h * options.scale,
            &[0, 0, 0, 0, 0, 0],
        )?;
        encoder.set_repeat(gif::Repeat::Infinite)?;
        for index in 0..frames {
            let mut pixels = Vec::with_capacity(usize::from(w) * usize::from(h));
            for y in 0..usize::from(h) {
                for x in 0..usize::from(w) {
                    let c = color_at(index, x, y);
                    pixels.push([c.red, c.green, c.blue, c.alpha]);
                }
            }
            let mut frame = palette_frame(&pixels, w, h, options);
            // 累计时间量化到 GIF 的 10ms 单位，避免每帧独立取整导致持续漂移。
            frame.delay = (((index + 1) as f64 * 100.0 / fps).round()
                - (index as f64 * 100.0 / fps).round()) as u16;
            frame.dispose = gif::DisposalMethod::Background;
            encoder.write_frame(&frame)?;
        }
        encoder.into_inner()?;
        Ok(())
    })
}

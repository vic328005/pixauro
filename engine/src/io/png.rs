//! 从当前作品生成 PNG，不修改项目、历史、保存基准或显示版本。
//! 导出使用原始 RGBA，棋盘格、界面缩放和工具预览不属于图片内容。

use std::io::Write;
use std::path::Path;

use super::atomic_file::{WriteMode, write_atomic};

use crate::{Color, EditError, Engine, FrameId, LayerId, ProjectError};

impl Engine {
    /// 按画布原始尺寸导出当前正式内容，包括尚未保存到 JSON 的修改。
    /// 可见图层按普通 Alpha 合成，图层不透明度与调色板 Alpha 相乘。
    pub fn export_png(&self, path: impl AsRef<Path>) -> Result<(), EditError> {
        self.export_frame_png(self.project().canvas().frames()[0].id, path)
    }

    pub fn export_frame_png(
        &self,
        frame: FrameId,
        path: impl AsRef<Path>,
    ) -> Result<(), EditError> {
        let canvas = self.project().canvas();
        canvas.frame(frame)?;
        write_png(
            path.as_ref(),
            u32::from(canvas.width()),
            u32::from(canvas.height()),
            |x, y| {
                self.project()
                    .composite_frame_at(frame, y * usize::from(canvas.width()) + x)
            },
        )?;
        Ok(())
    }

    /// 导出单个图层；忽略 visible，保留 opacity，不裁切、不混入其他层。
    pub fn export_layer_png(&self, id: LayerId, path: impl AsRef<Path>) -> Result<(), EditError> {
        let project = self.project();
        let canvas = project.canvas();
        let layer = canvas.layer(id)?;
        let frame = canvas.animations()[canvas.animation(layer.animation())?].frames[0].frame;
        write_png(
            path.as_ref(),
            u32::from(canvas.width()),
            u32::from(canvas.height()),
            |x, y| {
                let mut color = layer
                    .pixel_at(frame, (x as i32, y as i32))
                    .and_then(|id| project.palette().color(id))
                    .unwrap_or(Color::rgba(0, 0, 0, 0));
                color.alpha = ((u32::from(color.alpha) * u32::from(layer.properties().opacity)
                    + 127)
                    / 255) as u8;
                color
            },
        )?;
        Ok(())
    }

    /// 按显示顺序从左到右、从上到下导出全部调色板条目，保留重复色和未使用的颜色。
    /// 每色 16×16 像素，每行最多 16 色；末行未使用位置透明，不添加间距或文字。
    pub fn export_palette_png(&self, path: impl AsRef<Path>) -> Result<(), EditError> {
        const SWATCH_SIZE: usize = 16;
        const MAX_COLUMNS: usize = 16;
        let colors: Vec<_> = self
            .project()
            .palette()
            .entries()
            .map(|(_, color)| color)
            .collect();
        // Palette 保证至少包含一种颜色，因此列数始终非零。
        let columns = colors.len().min(MAX_COLUMNS);
        let rows = colors.len().div_ceil(columns);
        let height = u32::try_from(rows)
            .ok()
            .and_then(|rows| rows.checked_mul(SWATCH_SIZE as u32))
            .ok_or_else(|| ProjectError::InvalidData("调色板过大，无法导出 PNG".into()))?;
        write_png(
            path.as_ref(),
            (columns * SWATCH_SIZE) as u32,
            height,
            |x, y| {
                let index = (y / SWATCH_SIZE) * columns + x / SWATCH_SIZE;
                colors
                    .get(index)
                    .copied()
                    .unwrap_or(Color::rgba(0, 0, 0, 0))
            },
        )?;
        Ok(())
    }
}

/// PNG 导出接口共用编码与文件写入。按行填充 RGBA，避免为大调色板的放大色块
/// 分配整张图片的像素缓冲；宽度来自画布或固定色块布局，行缓冲始终很小。
pub(super) fn write_png(
    path: &Path,
    width: u32,
    height: u32,
    color_at: impl Fn(usize, usize) -> Color,
) -> Result<(), ProjectError> {
    write_atomic(path, WriteMode::Replace, |file| {
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        let mut stream = writer.stream_writer()?;
        let mut row = vec![0; width as usize * 4];
        for y in 0..height as usize {
            for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let color = color_at(x, y);
                // RGB 不预乘 alpha，也不与白底或棋盘格混合。
                pixel.copy_from_slice(&[color.red, color.green, color.blue, color.alpha]);
            }
            stream.write_all(&row)?;
        }
        // 显式结束压缩流和 PNG，确保结束块/刷新失败能返回给调用方。
        stream.finish()?;
        writer.finish()?;
        Ok(())
    })
}

/// 供独立资源流水线写入 RGBA；尺寸和缓冲由流水线校验。
pub(crate) fn write_raster_png(path: &Path, raster: &crate::PngRaster) -> Result<(), ProjectError> {
    write_png(
        path,
        u32::from(raster.width),
        u32::from(raster.height),
        |x, y| {
            let i = (y * usize::from(raster.width) + x) * 4;
            Color::rgba(
                raster.rgba[i],
                raster.rgba[i + 1],
                raster.rgba[i + 2],
                raster.rgba[i + 3],
            )
        },
    )
}

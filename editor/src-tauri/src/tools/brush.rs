use std::collections::BTreeSet;

use pixauro_engine::commands::SetPixelsCommand;
use pixauro_engine::{ColorId, Project};

/// 一笔尚未提交的绘制或擦除。只保存覆盖位置，不修改 Engine 的项目。
/// 大小与目标像素值在落笔时固定；多次经过同一位置不会累积透明度。
pub struct BrushStroke {
    width: u16,
    height: u16,
    size: u16,
    // Some(id) 绘画，None 擦除。目标在落笔时固定，整笔始终使用同一种操作。
    target: Option<ColorId>,
    last: Option<(i32, i32)>,
    pixels: BTreeSet<usize>,
}

impl BrushStroke {
    pub fn new(
        project: &Project,
        point: (i32, i32),
        size: u16,
        target: Option<ColorId>,
    ) -> Result<Self, String> {
        if size == 0 {
            return Err("画笔边长必须大于零".into());
        }
        if let Some(id) = target
            && project.palette().color(id).is_none()
        {
            return Err(format!("调色板中不存在颜色 ID {id}"));
        }
        let mut stroke = Self {
            width: project.canvas().width(),
            height: project.canvas().height(),
            size,
            target,
            last: None,
            pixels: BTreeSet::new(),
        };
        stroke.append(Some(point))?;
        Ok(stroke)
    }

    /// None 中断连线，重入画布时从新点开始；返回覆盖范围是否增长。
    pub fn append(&mut self, point: Option<(i32, i32)>) -> Result<bool, String> {
        if let Some((x, y)) = point {
            if x < 0 || y < 0 || x >= i32::from(self.width) || y >= i32::from(self.height) {
                return Err(format!("画笔坐标 ({x}, {y}) 位于画布外"));
            }
            if self.last == point {
                return Ok(false);
            }
            let count = self.pixels.len();
            self.draw_line(self.last.unwrap_or((x, y)), (x, y));
            self.last = point;
            Ok(self.pixels.len() != count)
        } else {
            self.last = None;
            Ok(false)
        }
    }

    /// 收笔后把工具的结果转换为通用命令，Engine 不需要知道笔画路径或大小。
    pub fn into_command(self) -> SetPixelsCommand {
        let width = usize::from(self.width);
        SetPixelsCommand::new(
            self.pixels
                .into_iter()
                .map(|offset| {
                    (
                        ((offset % width) as i32, (offset / width) as i32),
                        self.target,
                    )
                })
                .collect(),
        )
    }

    /// Bresenham 补齐快速拖动的路径，再对每个点盖上方形笔刷。
    /// 覆盖范围先裁剪后转换为数组下标；偶数大小偏向右下，与原画笔一致。
    fn draw_line(&mut self, from: (i32, i32), to: (i32, i32)) {
        let size = i32::from(self.size);
        let offset = (size - 1) / 2;
        let (mut x, mut y) = from;
        let dx = (to.0 - x).abs();
        let dy = -(to.1 - y).abs();
        let sx = if x < to.0 { 1 } else { -1 };
        let sy = if y < to.1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            for py in (y - offset).max(0)..(y - offset + size).min(i32::from(self.height)) {
                for px in (x - offset).max(0)..(x - offset + size).min(i32::from(self.width)) {
                    self.pixels
                        .insert(py as usize * usize::from(self.width) + px as usize);
                }
            }
            if (x, y) == to {
                break;
            }
            let twice = 2 * error;
            if twice >= dy {
                error += dy;
                x += sx;
            }
            if twice <= dx {
                error += dx;
                y += sy;
            }
        }
    }
}

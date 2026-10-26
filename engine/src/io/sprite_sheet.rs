//! 面向外部播放器的完整画布精灵图及动画描述，不包含编辑历史。
use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use super::{
    atomic_file::{WriteMode, write_bytes},
    png::write_png,
};
use crate::{AnimationId, Color, EditError, Engine, ProjectError};

const MAX_SIDE: usize = 8192;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SpriteSheet<'a> {
    format: &'static str,
    version: u32,
    image: String,
    width: usize,
    height: usize,
    frame_width: usize,
    frame_height: usize,
    columns: usize,
    rows: usize,
    frames: Vec<SheetFrame>,
    animations: Vec<SheetAnimation<'a>>,
}

#[derive(Serialize)]
struct SheetFrame {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

#[derive(Serialize)]
struct SheetAnimation<'a> {
    name: &'a str,
    fps: f64,
    frames: Vec<usize>,
}

impl Engine {
    /// None 导出全部动画；Some 导出指定动画的全部帧。
    /// 在 parent 内新建不重名文件夹，返回其中的 JSON 路径，不覆盖已有导出。
    pub fn export_sprite_sheet(
        &self,
        animation: Option<AnimationId>,
        parent: impl AsRef<Path>,
    ) -> Result<PathBuf, EditError> {
        let canvas = self.project().canvas();
        let animations = match animation {
            Some(id) => std::slice::from_ref(&canvas.animations()[canvas.animation(id)?]),
            None => canvas.animations(),
        };
        let frame_ids: Vec<_> = animations
            .iter()
            .flat_map(|a| a.frames.iter().map(|f| f.frame))
            .collect();
        let w = usize::from(canvas.width());
        let h = usize::from(canvas.height());
        write_sheet(
            self.project().name(),
            w,
            h,
            animations
                .iter()
                .map(|a| (a.name.as_str(), a.fps, a.frames.len()))
                .collect(),
            |index, x, y| {
                self.project()
                    .composite_frame_at(frame_ids[index], y * w + x)
            },
            parent.as_ref(),
        )
    }
}

/// 资源流水线不创建临时 Engine 或历史，和编辑器使用同一格式及原子输出实现。
pub(crate) fn export_animation_asset(
    name: &str,
    width: u16,
    height: u16,
    animations: &[crate::SpriteAnimation],
    parent: &Path,
) -> Result<PathBuf, EditError> {
    let frames: Vec<_> = animations.iter().flat_map(|a| &a.frames).collect();
    write_sheet(
        name,
        usize::from(width),
        usize::from(height),
        animations
            .iter()
            .map(|a| (a.name.as_str(), a.fps, a.frames.len()))
            .collect(),
        |index, x, y| {
            let frame = frames[index];
            let x = x as i32 - frame.x;
            let y = y as i32 - frame.y;
            if x < 0 || y < 0 || x >= i32::from(frame.width) || y >= i32::from(frame.height) {
                return Color::rgba(0, 0, 0, 0);
            }
            let i = (y as usize * usize::from(frame.width) + x as usize) * 4;
            Color::rgba(
                frame.rgba[i],
                frame.rgba[i + 1],
                frame.rgba[i + 2],
                frame.rgba[i + 3],
            )
        },
        parent,
    )
}

fn write_sheet(
    name: &str,
    w: usize,
    h: usize,
    animations: Vec<(&str, f64, usize)>,
    color_at: impl Fn(usize, usize, usize) -> Color,
    parent: &Path,
) -> Result<PathBuf, EditError> {
    let frame_count: usize = animations.iter().map(|a| a.2).sum();
    // 遍历合法列数，先让最长边最小，再减少末行空白。
    let (columns, rows) = (1..=frame_count.min(MAX_SIDE / w))
        .map(|columns| (columns, frame_count.div_ceil(columns)))
        .filter(|&(_, rows)| rows * h <= MAX_SIDE)
        .min_by_key(|&(columns, rows)| ((columns * w).max(rows * h), columns * rows))
        .ok_or_else(|| {
            ProjectError::InvalidData(
                "动画无法放入一张 8192×8192 以内的贴图，请单独导出动画或减小画布、帧数。".into(),
            )
        })?;
    let mut offset = 0;
    let name = export_name(name);
    let sheet = SpriteSheet {
        format: "pixauro-spritesheet",
        version: 1,
        image: format!("{name}.png"),
        width: columns * w,
        height: rows * h,
        frame_width: w,
        frame_height: h,
        columns,
        rows,
        frames: (0..frame_count)
            .map(|i| SheetFrame {
                x: i % columns * w,
                y: i / columns * h,
                w,
                h,
            })
            .collect(),
        animations: animations
            .into_iter()
            .map(|(name, fps, count)| {
                let frames = (offset..offset + count).collect();
                offset += count;
                SheetAnimation { name, fps, frames }
            })
            .collect(),
    };
    let json = serde_json::to_vec_pretty(&sheet).map_err(ProjectError::from)?;
    // create_dir 排他预留本次输出目录；JSON 引用实际的 PNG 文件名。
    let directory = new_export_directory(parent, &name)?;
    let png_path = directory.join(&sheet.image);
    let json_path = directory.join(format!("{name}.json"));
    let mut png_written = false;
    let result = (|| -> Result<(), ProjectError> {
        write_png(
            &png_path,
            sheet.width as u32,
            sheet.height as u32,
            |x, y| {
                let index = y / h * columns + x / w;
                if index < frame_count {
                    color_at(index, x % w, y % h)
                } else {
                    Color::rgba(0, 0, 0, 0)
                }
            },
        )?;
        png_written = true;
        write_bytes(&json_path, &json, WriteMode::CreateNew)
    })();
    if let Err(error) = result {
        // JSON 原子写入失败不会留下目标；仅清理本次已成功写入的 PNG。
        if png_written
            && let Err(cleanup) = fs::remove_file(&png_path)
            && cleanup.kind() != std::io::ErrorKind::NotFound
        {
            return Err(ProjectError::InvalidData(format!(
                "{error}；清理未完成的导出失败（{}）：{cleanup}",
                directory.display()
            ))
            .into());
        }
        if let Err(cleanup) = fs::remove_dir(&directory) {
            return Err(ProjectError::InvalidData(format!(
                "{error}；清理导出文件夹失败（{}）：{cleanup}",
                directory.display()
            ))
            .into());
        }
        return Err(error.into());
    }
    Ok(json_path)
}

/// 保留中文等项目名字符，限制长度并处理跨平台文件名限制。
pub(crate) fn export_name(project_name: &str) -> String {
    let name: String = project_name
        .chars()
        .take(32)
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim_matches(|c: char| c.is_whitespace() || c == '.');
    if name.is_empty() {
        return "project".into();
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|number| {
                matches!(
                    number,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
    if reserved {
        format!("_{name}")
    } else {
        name.into()
    }
}

fn new_export_directory(parent: &Path, name: &str) -> Result<PathBuf, ProjectError> {
    for suffix in 0..10_000 {
        let name = if suffix == 0 {
            name.to_owned()
        } else {
            format!("{name}-{suffix}")
        };
        let directory = parent.join(name);
        match fs::create_dir(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(ProjectError::InvalidData(
        "导出文件夹重名过多，请选择其他位置。".into(),
    ))
}

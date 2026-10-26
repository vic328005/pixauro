//! 无编辑器会话的资源处理：配置、切片、定位、去背、帧组装及标准导出。
mod locate;
mod pipeline;

pub use locate::{
    LocateAxis, LocateIssue, LocateItem, LocateRequest, LocateStatus, auto_locate_slices,
};
pub use pipeline::{
    AnimationAsset, AnimationRecipe, AssetOptions, BackgroundMode, PixelImage, SliceLayout,
    animate, pixelate,
};

use crate::{
    Animation, PngRaster, ProjectError, SpriteAnimation, SpriteFrame, SpritePreview, SpriteSlice,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(crate) fn invalid(message: impl Into<String>) -> ProjectError {
    ProjectError::InvalidData(message.into())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportSlice {
    pub id: String,
    pub row: u16,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub enabled: bool,
    pub auto_locked: bool,
}
impl Default for ImportSlice {
    fn default() -> Self {
        Self {
            id: String::new(),
            row: 0,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            offset_x: 0,
            offset_y: 0,
            enabled: true,
            auto_locked: false,
        }
    }
}
impl ImportSlice {
    pub fn region(&self) -> SpriteSlice {
        SpriteSlice {
            id: self.id.clone(),
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct ImportGroup {
    pub name: String,
    pub fps: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(default, deny_unknown_fields)]
pub struct SliceGrid {
    pub rows: u16,
    pub columns: u16,
    pub margin: u32,
    pub gap: u32,
}
impl Default for SliceGrid {
    fn default() -> Self {
        Self {
            rows: 1,
            columns: 1,
            margin: 0,
            gap: 0,
        }
    }
}

pub fn grid_slices(
    width: u32,
    height: u32,
    grid: SliceGrid,
) -> Result<Vec<ImportSlice>, ProjectError> {
    let SliceGrid {
        rows,
        columns,
        margin,
        gap,
    } = grid;
    if rows == 0 || columns == 0 || u32::from(rows) * u32::from(columns) > 256 {
        return Err(invalid("网格需要 1～256 个切片"));
    }
    let available = |size: u32, count: u16| {
        u64::from(size)
            .checked_sub(u64::from(margin) * 2 + u64::from(gap) * u64::from(count - 1))
            .filter(|&v| v >= u64::from(count))
            .ok_or_else(|| invalid("网格无法放入原图，请减小行列数、外边距或间距"))
    };
    let aw = available(width, columns)?;
    let ah = available(height, rows)?;
    // 正数四舍五入，与 UI 的原网格边界约定相同；整数运算避免累计误差。
    let boundary = |available: u64, index: u16, count: u16| {
        ((available * u64::from(index) * 2 + u64::from(count)) / (u64::from(count) * 2)) as u32
    };
    Ok((0..rows)
        .flat_map(|row| {
            (0..columns).map(move |col| {
                let x = boundary(aw, col, columns);
                let y = boundary(ah, row, rows);
                ImportSlice {
                    id: format!("{row}:{col}"),
                    row,
                    x: margin + u32::from(col) * gap + x,
                    y: margin + u32::from(row) * gap + y,
                    width: boundary(aw, col + 1, columns) - x,
                    height: boundary(ah, row + 1, rows) - y,
                    offset_x: 0,
                    offset_y: 0,
                    enabled: true,
                    auto_locked: false,
                }
            })
        })
        .collect())
}

pub(crate) fn validate_slices(
    slices: &[ImportSlice],
    width: u32,
    height: u32,
) -> Result<(), ProjectError> {
    if slices.is_empty() || slices.len() > 256 {
        return Err(invalid("切片数量必须为 1～256"));
    }
    let mut ids = HashSet::new();
    for s in slices {
        if s.id.is_empty()
            || s.id.len() > 128
            || !ids.insert(&s.id)
            || s.row >= 256
            || s.width == 0
            || s.height == 0
            || s.x.checked_add(s.width).is_none_or(|v| v > width)
            || s.y.checked_add(s.height).is_none_or(|v| v > height)
            || s.offset_x.abs_diff(0) > 8192
            || s.offset_y.abs_diff(0) > 8192
        {
            return Err(invalid(format!(
                "切片 {} 的 ID、范围、分组或偏移无效",
                s.id
            )));
        }
    }
    Ok(())
}

#[derive(Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "bindings", derive(specta::Type))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportAssembly {
    pub slices: Vec<ImportSlice>,
    pub groups: Vec<ImportGroup>,
    pub remove_background: bool,
    // 0=自动，1=移除，2=保留；坐标对应像素化后的帧。
    #[serde(default)]
    pub overrides: BTreeMap<String, Vec<u8>>,
}

pub fn clean_raster(
    raster: &mut PngRaster,
    remove_background: bool,
    overrides: Option<&[u8]>,
) -> Result<(), ProjectError> {
    let area = usize::from(raster.width) * usize::from(raster.height);
    if area == 0
        || raster.width > 4096
        || raster.height > 4096
        || raster.rgba.len() != area * 4
        || raster
            .foreground
            .as_ref()
            .is_some_and(|m| m.len() != area || m.iter().any(|&v| v > 1))
        || overrides.is_some_and(|m| m.len() != area || m.iter().any(|&v| v > 2))
    {
        return Err(invalid("帧尺寸、RGBA 或去背蒙版无效"));
    }
    for (i, pixel) in raster.rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let correction = overrides.map_or(0, |m| m[i]);
        if correction == 1
            || (remove_background
                && correction != 2
                && raster.foreground.as_ref().is_some_and(|m| m[i] == 0))
        {
            pixel[3] = 0;
        }
    }
    Ok(())
}

/// 桌面最终提交与 CLI 共用；预览可保留未去背 RGBA 以便交互修正。
pub fn assemble_sprite_import(
    preview: SpritePreview,
    assembly: &ImportAssembly,
    canvas_size: (u16, u16),
) -> Result<Vec<SpriteAnimation>, ProjectError> {
    if canvas_size.0 == 0 || canvas_size.1 == 0 || canvas_size.0 > 4096 || canvas_size.1 > 4096 {
        return Err(invalid("输出画布宽高必须为 1～4096"));
    }
    validate_slices(
        &assembly.slices,
        preview.source_width,
        preview.source_height,
    )?;
    if assembly.groups.is_empty() || assembly.groups.len() > 256 {
        return Err(invalid("动画分组数量无效"));
    }
    let enabled: Vec<_> = assembly.slices.iter().filter(|s| s.enabled).collect();
    if enabled.is_empty() || preview.frames.len() != enabled.len() {
        return Err(invalid("预览帧数与已启用切片不一致"));
    }
    let mut rasters = HashMap::new();
    let mut total = 0_usize;
    for f in preview.frames {
        total = total.saturating_add(usize::from(f.raster.width) * usize::from(f.raster.height));
        if total > 4096 * 4096 || rasters.insert(f.id, f.raster).is_some() {
            return Err(invalid("重复预览 ID 或帧像素总量超过 4096²"));
        }
    }
    if enabled
        .iter()
        .any(|s| usize::from(s.row) >= assembly.groups.len() || !rasters.contains_key(&s.id))
        || assembly
            .overrides
            .keys()
            .any(|id| !rasters.contains_key(id))
    {
        return Err(invalid("预览、修正蒙版或分组与切片不匹配"));
    }
    let mut names = HashSet::new();
    let mut animations = Vec::new();
    for (row, group) in assembly.groups.iter().enumerate() {
        let list: Vec<_> = enabled
            .iter()
            .filter(|s| usize::from(s.row) == row)
            .collect();
        if list.is_empty() {
            continue;
        }
        let name = group.name.trim();
        if name.is_empty()
            || name.chars().count() > 128
            || name.chars().any(char::is_control)
            || !names.insert(name)
            || !Animation::valid_fps(group.fps)
        {
            return Err(invalid("动画名称重复或无效，或 FPS 不在 0.2～100 范围"));
        }
        let mut frames = Vec::new();
        for s in list {
            let mut raster = rasters.remove(&s.id).ok_or_else(|| invalid("缺少预览帧"))?;
            if raster.source_width != s.width || raster.source_height != s.height {
                return Err(invalid("切片尺寸与预览不一致，请重新生成预览"));
            }
            clean_raster(
                &mut raster,
                assembly.remove_background,
                assembly.overrides.get(&s.id).map(Vec::as_slice),
            )?;
            let x = (i32::from(canvas_size.0) - i32::from(raster.width)).div_euclid(2) + s.offset_x;
            let y =
                (i32::from(canvas_size.1) - i32::from(raster.height)).div_euclid(2) + s.offset_y;
            if x.abs_diff(0) > 8192 || y.abs_diff(0) > 8192 {
                return Err(invalid("最终帧偏移超出 ±8192"));
            }
            frames.push(SpriteFrame {
                width: raster.width,
                height: raster.height,
                x,
                y,
                rgba: raster.rgba,
            });
        }
        animations.push(SpriteAnimation {
            name: name.into(),
            fps: group.fps,
            frames,
        });
    }
    Ok(animations)
}

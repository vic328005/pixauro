use super::{
    ImportAssembly, ImportGroup, ImportSlice, LocateAxis, LocateRequest, LocateStatus, SliceGrid,
    assemble_sprite_import, auto_locate_slices, clean_raster, grid_slices, invalid,
    validate_slices,
};
use crate::{
    EditError, PngImportOptions, PngRaster, SpriteAnimation, SpriteImportProgress, SpriteSlice,
    inspect_png, png_source_preview,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ops::ControlFlow,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundMode {
    None,
    Source,
    #[default]
    Pixel,
    Both,
}
impl BackgroundMode {
    fn source(self) -> bool {
        matches!(self, Self::Source | Self::Both)
    }
    fn pixel(self) -> bool {
        matches!(self, Self::Pixel | Self::Both)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetOptions {
    pub canvas_width: u16,
    pub canvas_height: u16,
    /// null 保留原图裁切像素，不运行像素化；只适用于 animate。
    pub pixelation: Option<PngImportOptions>,
    pub background: BackgroundMode,
    pub source_background_edge: f32,
}
impl Default for AssetOptions {
    fn default() -> Self {
        Self {
            canvas_width: 64,
            canvas_height: 64,
            pixelation: Some(PngImportOptions::default()),
            background: BackgroundMode::Pixel,
            source_background_edge: 0.0,
        }
    }
}
impl AssetOptions {
    fn validate(&self) -> Result<(), crate::ProjectError> {
        if !(1..=4096).contains(&self.canvas_width)
            || !(1..=4096).contains(&self.canvas_height)
            || !self.source_background_edge.is_finite()
            || self.source_background_edge < 0.0
        {
            return Err(invalid("输出尺寸或去背边缘宽度无效"));
        }
        if let Some(options) = self.pixelation {
            options.validate()?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SliceLayout {
    Grid { grid: SliceGrid },
    Slices { slices: Vec<ImportSlice> },
}
impl Default for SliceLayout {
    fn default() -> Self {
        Self::Grid {
            grid: SliceGrid::default(),
        }
    }
}

/// 可保存和重复执行的配方；输入与输出路径由调用方提供。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct AnimationRecipe {
    pub options: AssetOptions,
    pub layout: SliceLayout,
    /// 空列表按行生成 Animation 1…，每行 10 FPS；显式列表按 row 索引。
    pub groups: Vec<ImportGroup>,
    pub auto_locate: Option<LocateAxis>,
    pub overrides: BTreeMap<String, Vec<u8>>,
}

pub struct AnimationAsset {
    width: u16,
    height: u16,
    animations: Vec<SpriteAnimation>,
    pub warnings: Vec<String>,
}
impl AnimationAsset {
    pub fn canvas_size(&self) -> (u16, u16) {
        (self.width, self.height)
    }
    pub fn animations(&self) -> &[SpriteAnimation] {
        &self.animations
    }
    pub fn into_animations(self) -> Vec<SpriteAnimation> {
        self.animations
    }
    /// 索引按 animations() 顺序，导出单个循环动画。
    pub fn export_gif(
        &self,
        animation: usize,
        path: impl AsRef<Path>,
        options: crate::GifExportOptions,
    ) -> Result<(), crate::ProjectError> {
        let animation = self
            .animations
            .get(animation)
            .ok_or_else(|| invalid("动画索引超出范围"))?;
        crate::io::write_animation_gif(
            path.as_ref(),
            (self.width, self.height),
            animation.frames.len(),
            animation.fps,
            options,
            |index, x, y| {
                let frame = &animation.frames[index];
                let x = x as i32 - frame.x;
                let y = y as i32 - frame.y;
                if x < 0 || y < 0 || x >= i32::from(frame.width) || y >= i32::from(frame.height) {
                    return crate::Color::rgba(0, 0, 0, 0);
                }
                let i = (y as usize * usize::from(frame.width) + x as usize) * 4;
                crate::Color::rgba(
                    frame.rgba[i],
                    frame.rgba[i + 1],
                    frame.rgba[i + 2],
                    frame.rgba[i + 3],
                )
            },
        )
    }
    /// 每个动画一个 GIF，序号避免重名；每个文件独立原子写入。
    pub fn export_gifs(
        &self,
        parent: impl AsRef<Path>,
        options: crate::GifExportOptions,
    ) -> Result<Vec<PathBuf>, crate::ProjectError> {
        for animation in &self.animations {
            options.validate(self.width, self.height, animation.frames.len())?;
        }
        let mut paths = Vec::new();
        for (index, animation) in self.animations.iter().enumerate() {
            let path = parent.as_ref().join(format!(
                "{:03}-{}.gif",
                index + 1,
                crate::io::export_name(&animation.name)
            ));
            self.export_gif(index, &path, options)?;
            paths.push(path);
        }
        Ok(paths)
    }
    pub fn export(&self, name: &str, parent: impl AsRef<Path>) -> Result<PathBuf, EditError> {
        crate::io::export_animation_asset(
            name,
            self.width,
            self.height,
            &self.animations,
            parent.as_ref(),
        )
    }
}

pub struct PixelImage {
    pub raster: PngRaster,
    pub warnings: Vec<String>,
}
impl PixelImage {
    pub fn export(&self, path: impl AsRef<Path>) -> Result<(), crate::ProjectError> {
        // 公共 raster 可被调用方修改，因此写入前重新校验。
        let mut raster = self.raster.clone();
        clean_raster(&mut raster, false, None)?;
        crate::io::write_raster_png(path.as_ref(), &raster)
    }
}
fn raster_warnings(id: &str, raster: &PngRaster, remove_background: bool) -> Vec<String> {
    let mut warnings = Vec::new();
    if !raster.converged {
        warnings.push(format!("切片 {id} 达到迭代上限，输出当前结果"));
    }
    if remove_background && raster.foreground.is_none() {
        warnings.push(format!("切片 {id} 无法可靠估计背景，保留原有透明度"));
    }
    warnings
}

/// 单图像素化，返回实际像素网格（保持宽高比、不放大），输出不要求创建项目。
pub fn pixelate(
    input: impl AsRef<Path>,
    options: &AssetOptions,
    progress: impl FnMut(SpriteImportProgress) -> ControlFlow<()>,
) -> Result<Option<PixelImage>, EditError> {
    options.validate()?;
    if options.pixelation.is_none() {
        return Err(invalid("pixelate 必须启用像素化").into());
    }
    let info = inspect_png(input.as_ref())?;
    let slices = [SpriteSlice {
        id: "image".into(),
        x: 0,
        y: 0,
        width: info.width,
        height: info.height,
    }];
    let Some(preview) = crate::io::prepare_sprite_rasters(
        input,
        &slices,
        (options.canvas_width, options.canvas_height),
        options.pixelation,
        options.background.source(),
        options.source_background_edge,
        progress,
    )?
    else {
        return Ok(None);
    };
    let mut raster = preview
        .frames
        .into_iter()
        .next()
        .ok_or_else(|| invalid("缺少图片处理结果"))?
        .raster;
    let warnings = raster_warnings("image", &raster, options.background.pixel());
    clean_raster(&mut raster, options.background.pixel(), None)?;
    Ok(Some(PixelImage { raster, warnings }))
}

/// 完整 spritesheet 处理；取消返回 None，所有输出留在内存直到调用 export。
pub fn animate(
    input: impl AsRef<Path>,
    recipe: &AnimationRecipe,
    mut progress: impl FnMut(SpriteImportProgress) -> ControlFlow<()>,
) -> Result<Option<AnimationAsset>, EditError> {
    let input = input.as_ref();
    let options = &recipe.options;
    options.validate()?;
    if progress(SpriteImportProgress {
        completed: 0,
        total: 0,
        active: 0,
    })
    .is_break()
    {
        return Ok(None);
    }
    let info = inspect_png(input)?;
    let mut slices = match &recipe.layout {
        SliceLayout::Grid { grid } => grid_slices(info.width, info.height, *grid)?,
        SliceLayout::Slices { slices } => slices.clone(),
    };
    validate_slices(&slices, info.width, info.height)?;
    let mut warnings = Vec::new();
    if let Some(axis) = recipe.auto_locate {
        let source = png_source_preview(
            input,
            options.background.source(),
            options.source_background_edge,
        )?;
        let request = LocateRequest {
            width: source.width,
            height: source.height,
            source_width: source.source_width,
            source_height: source.source_height,
            rgba: source.rgba,
            slices: slices.clone(),
            axis,
        };
        let total = slices.iter().filter(|s| s.enabled).count() as u32;
        let Some(items) = auto_locate_slices(&request, |_| {
            progress(SpriteImportProgress {
                completed: 0,
                total,
                active: 0,
            })
        })?
        else {
            return Ok(None);
        };
        for (slice, item) in slices.iter_mut().zip(items) {
            if item.status == LocateStatus::Moved {
                slice.x = item.x;
                slice.y = item.y;
            }
            if item.status == LocateStatus::Review {
                warnings.push(format!("切片 {} 定位需人工检查：{:?}", item.id, item.issue));
            }
        }
    }
    let groups = if recipe.groups.is_empty() {
        (0..=slices.iter().map(|s| s.row).max().unwrap_or(0))
            .map(|row| ImportGroup {
                name: format!("Animation {}", row + 1),
                fps: 10.0,
            })
            .collect()
    } else {
        recipe.groups.clone()
    };
    let regions: Vec<_> = slices
        .iter()
        .filter(|s| s.enabled)
        .map(ImportSlice::region)
        .collect();
    let Some(preview) = crate::io::prepare_sprite_rasters(
        input,
        &regions,
        (options.canvas_width, options.canvas_height),
        options.pixelation,
        options.background.source(),
        options.source_background_edge,
        &mut progress,
    )?
    else {
        return Ok(None);
    };
    for frame in &preview.frames {
        warnings.extend(raster_warnings(
            &frame.id,
            &frame.raster,
            options.background.pixel(),
        ));
    }
    let assembly = ImportAssembly {
        slices,
        groups,
        remove_background: options.background.pixel(),
        overrides: recipe.overrides.clone(),
    };
    let animations = assemble_sprite_import(
        preview,
        &assembly,
        (options.canvas_width, options.canvas_height),
    )?;
    for animation in &animations {
        for (index, frame) in animation.frames.iter().enumerate() {
            if frame.x < 0
                || frame.y < 0
                || frame.x + i32::from(frame.width) > i32::from(options.canvas_width)
                || frame.y + i32::from(frame.height) > i32::from(options.canvas_height)
            {
                warnings.push(format!(
                    "动画 {} 第 {} 帧超出画布，导出将裁切；请调整画布或偏移",
                    animation.name,
                    index + 1
                ));
            }
        }
    }
    if progress(SpriteImportProgress {
        completed: regions.len() as u32,
        total: regions.len() as u32,
        active: 0,
    })
    .is_break()
    {
        return Ok(None);
    }
    Ok(Some(AnimationAsset {
        width: options.canvas_width,
        height: options.canvas_height,
        animations,
        warnings,
    }))
}

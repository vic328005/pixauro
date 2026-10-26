//! 两种资源导入共用的配色规则；空作品首次导入时采用资源配色。
use crate::{Color, ColorId, EditError, Palette, PaletteSource, Project};
use std::collections::HashMap;

pub(super) struct ImportPalette {
    palette: Palette,
    colors: HashMap<[u8; 4], ColorId>,
    replaced: Vec<ColorId>,
}

impl ImportPalette {
    pub(super) fn new(project: &Project) -> Self {
        let palette = project.palette().clone();
        let replace = project
            .canvas()
            .layers()
            .iter()
            .all(|layer| layer.images().values().all(|pixels| pixels.is_empty()))
            && palette
                .entries()
                .map(|(id, color)| (color, palette.source(id)))
                .eq(Palette::default()
                    .entries()
                    .map(|(_, color)| (color, Some(PaletteSource::Custom))));
        let replaced = if replace {
            palette.entries().map(|(id, _)| id).collect()
        } else {
            Vec::new()
        };
        let colors = palette
            .entries()
            .filter(|(id, _)| palette.source(*id) == Some(PaletteSource::External))
            .map(|(id, c)| ([c.red, c.green, c.blue, c.alpha], id))
            .collect();
        Self {
            palette,
            colors,
            replaced,
        }
    }

    pub(super) fn color(&mut self, color: Color) -> Result<ColorId, EditError> {
        let key = [color.red, color.green, color.blue, color.alpha];
        Ok(match self.colors.entry(key) {
            std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let source = if self.replaced.is_empty() {
                    PaletteSource::External
                } else {
                    PaletteSource::Custom
                };
                *entry.insert(self.palette.append(color, source)?)
            }
        })
    }

    pub(super) fn finish(mut self) -> Result<Palette, EditError> {
        // 全透明资源没有可用配色；有颜色时才移除默认色，并保留 ID 分配范围供历史恢复。
        if !self.colors.is_empty() {
            for id in self.replaced {
                self.palette.remove(id)?;
            }
        }
        Ok(self.palette)
    }
}

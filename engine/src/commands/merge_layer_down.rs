//! 当前动画全部帧向下合并及其调色板、图层历史。
use super::{canvas_change::CanvasChange, invalid};
use crate::commands::command::{CommandState, sealed};
use crate::model::PaletteFile;
use crate::{Canvas, Cel, DrawError};
use crate::{Command, EditError, LayerId, Palette, PaletteSource, Project, ProjectError};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeLayerDownCommand {
    id: LayerId,
    label: String,
    change: Option<CanvasChange>,
    before: Option<PaletteFile>,
    after: Option<PaletteFile>,
    #[serde(skip)]
    state: CommandState,
}
impl MergeLayerDownCommand {
    pub fn new(id: LayerId) -> Self {
        Self {
            id,
            label: String::new(),
            change: None,
            before: None,
            after: None,
            state: CommandState::Fresh,
        }
    }
    fn apply(&self, p: &mut Project, forward: bool) -> Result<(), EditError> {
        let data = if forward { &self.after } else { &self.before };
        let mut palette = Palette::from_file(
            data.clone()
                .ok_or_else(|| ProjectError::InvalidData("合并历史缺少颜色".into()))?,
        )?;
        palette.preserve_ids(p.palette());
        let mut canvas = p.canvas().clone();
        self.change
            .as_ref()
            .ok_or_else(|| ProjectError::InvalidData("合并历史缺少图层".into()))?
            .apply(&mut canvas, &palette, forward)?;
        *p.canvas_mut() = canvas;
        *p.palette_mut() = palette;
        Ok(())
    }
}
impl sealed::Sealed for MergeLayerDownCommand {
    fn restore(&mut self, p: &Project, applied: bool) -> Result<(), EditError> {
        for data in [&self.before, &self.after] {
            Palette::from_file(
                data.clone()
                    .ok_or_else(|| ProjectError::InvalidData("合并历史无效".into()))?,
            )?;
        }
        self.change
            .as_ref()
            .ok_or_else(|| ProjectError::InvalidData("合并历史无效".into()))?
            .validate(p)?;
        self.state = CommandState::restored(applied);
        Ok(())
    }
}
#[typetag::serde(name = "merge_layer_down")]
impl Command for MergeLayerDownCommand {
    fn name(&self) -> &str {
        &self.label
    }
    fn execute(&mut self, p: &mut Project) -> Result<bool, EditError> {
        self.state.require(CommandState::Fresh, "执行")?;
        let mut c = p.canvas().clone();
        let mut palette = p.palette().clone();
        self.label = format!(
            "向下合并 · {} · 当前动画全部帧",
            c.layer(self.id)?.properties().name
        );
        merge(&mut c, &mut palette, self.id)?;
        c.validate(&palette, false)?;
        self.change = Some(CanvasChange::new(p.canvas(), &c));
        self.before = Some(p.palette().to_file());
        self.after = Some(palette.to_file());
        *p.canvas_mut() = c;
        *p.palette_mut() = palette;
        self.state = CommandState::Applied;
        Ok(true)
    }
    fn undo(&mut self, p: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Applied, "撤销")?;
        self.apply(p, false)?;
        self.state = CommandState::Undone;
        Ok(())
    }
    fn redo(&mut self, p: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Undone, "重做")?;
        self.apply(p, true)?;
        self.state = CommandState::Applied;
        Ok(())
    }
}

/// 每帧独立合成，按颜色 ID 和局部像素去重，只在合并后的同层、同动画内恢复链接。
fn merge(c: &mut Canvas, palette: &mut Palette, id: LayerId) -> Result<(), EditError> {
    let top = c
        .layers
        .iter()
        .position(|l| l.id == id)
        .ok_or(DrawError::InvalidLayerId(id))?;
    let owner = c.layers[top].animation;
    let bottom = c.layers[..top]
        .iter()
        .rposition(|l| l.animation == owner)
        .ok_or_else(|| invalid("当前动画最底层不能向下合并"))?;
    let pair = [c.layers[bottom].clone(), c.layers[top].clone()];
    for l in &pair {
        super::require_unlocked(c, l.id)?;
        if !l.properties.visible {
            return Err(invalid("请先显示两个图层再合并"));
        }
    }
    let mut lower = pair[0].clone();
    lower.cels.clear();
    lower.images.clear();
    lower.properties.opacity = 255;
    let mut signatures: BTreeMap<(crate::AnimationId, crate::model::ImagePixels), crate::ImageId> =
        BTreeMap::new();
    let frames = c.animations[c.animation(owner)?].frames.clone();
    for frame in frames {
        let mut positions = BTreeSet::new();
        for layer in &pair {
            if let Some(cel) = layer.cels.get(&frame.frame) {
                for &((x, y), _) in layer.images[&cel.image].iter() {
                    let pos = (x + cel.x, y + cel.y);
                    if c.offset(pos).is_ok() {
                        positions.insert(pos);
                    }
                }
            }
        }
        let mut pixels = Vec::new();
        for pos in positions {
            let color = crate::render::composite_layers(palette, &pair, frame.frame, pos);
            if color.alpha == 0 {
                continue;
            }
            let found = palette
                .entries()
                .find(|(_, v)| *v == color)
                .map(|(id, _)| id);
            let color_id = match found {
                Some(id) => id,
                None => palette.append(color, PaletteSource::External)?,
            };
            pixels.push((pos, color_id));
        }
        if pixels.is_empty() {
            continue;
        }
        let key = (owner, pixels.clone());
        let image = match signatures.get(&key) {
            Some(id) => *id,
            None => {
                let id = c.allocate_image()?;
                signatures.insert(key, id);
                lower.images.insert(id, Arc::new(pixels));
                id
            }
        };
        lower.cels.insert(frame.frame, Cel { image, x: 0, y: 0 });
    }
    c.layers[bottom] = lower;
    c.layers.remove(top);
    Ok(())
}

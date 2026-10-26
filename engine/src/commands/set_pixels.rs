use crate::commands::command::{CommandState, sealed, validate_color, validate_recorded_color};
use crate::{
    Cel, ColorId, Command, DrawError, EditError, FrameId, ImageId, LayerId, Project, ProjectError,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

type Pixel = ((i32, i32), Option<ColorId>);
type Delta = ((i32, i32), Option<ColorId>, Option<ColorId>);
/// 一笔只保存实际改变的像素，坐标从画布转换为共享图像的局部坐标。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetPixelsCommand {
    layer_id: LayerId,
    frame_id: FrameId,
    label: String,
    #[serde(skip)]
    pixels: Vec<Pixel>,
    changes: Vec<Delta>,
    image: Option<ImageId>,
    created: bool,
    #[serde(skip)]
    state: CommandState,
}
impl SetPixelsCommand {
    pub fn new(layer_id: LayerId, frame_id: FrameId, pixels: Vec<Pixel>) -> Self {
        Self {
            layer_id,
            frame_id,
            pixels,
            label: String::new(),
            changes: Vec::new(),
            image: None,
            created: false,
            state: CommandState::Fresh,
        }
    }
    fn apply(&self, project: &mut Project, forward: bool) -> Result<(), EditError> {
        let mut next = project.clone();
        self.apply_inner(&mut next, forward)?;
        next.canvas().validate(next.palette(), false)?;
        *project = next;
        Ok(())
    }
    fn apply_inner(&self, project: &mut Project, forward: bool) -> Result<(), EditError> {
        let image = self
            .image
            .ok_or_else(|| ProjectError::InvalidData("笔画缺少图像身份".into()))?;
        project
            .canvas()
            .require_layer_frame(self.layer_id, self.frame_id)?;
        let layer = project.canvas().layer(self.layer_id)?;
        if !(self.created && forward) && !layer.images.contains_key(&image) {
            return Err(ProjectError::InvalidData("笔画图像不存在".into()).into());
        }
        for &(_, before, after) in &self.changes {
            validate_color(project, if forward { after } else { before })?;
        }
        let mut pixels: BTreeMap<_, _> = layer
            .images
            .get(&image)
            .map(|p| p.iter().copied().collect())
            .unwrap_or_default();
        for &(position, before, after) in &self.changes {
            match if forward { after } else { before } {
                Some(id) => {
                    pixels.insert(position, id);
                }
                None => {
                    pixels.remove(&position);
                }
            }
        }
        let layer = project.canvas_mut().layer_mut(self.layer_id)?;
        if self.created && !forward {
            layer.cels.remove(&self.frame_id);
            layer.images.remove(&image);
        } else {
            if self.created {
                layer.cels.insert(self.frame_id, Cel { image, x: 0, y: 0 });
            }
            layer
                .images
                .insert(image, Arc::new(pixels.into_iter().collect()));
        }
        Ok(())
    }
}
impl sealed::Sealed for SetPixelsCommand {
    fn restore(&mut self, project: &Project, applied: bool) -> Result<(), EditError> {
        let canvas = project.canvas();
        if !canvas.was_allocated(self.layer_id)
            || self
                .image
                .is_none_or(|id| id.0 == 0 || id.0 >= canvas.next_animation_id)
            || self.frame_id.0 == 0
            || self.frame_id.0 >= canvas.next_animation_id
            || self.changes.len() > canvas.pixel_count()
            || self.changes.windows(2).any(|p| p[0].0 >= p[1].0)
        {
            return Err(ProjectError::InvalidData("笔画恢复数据无效".into()).into());
        }
        for &((x, y), before, after) in &self.changes {
            if x.abs_diff(0) > 12288 || y.abs_diff(0) > 12288 {
                return Err(ProjectError::InvalidData("笔画像素越界".into()).into());
            }
            validate_recorded_color(project, before)?;
            validate_recorded_color(project, after)?;
        }
        self.state = CommandState::restored(applied);
        Ok(())
    }
}
#[typetag::serde(name = "set_pixels")]
impl Command for SetPixelsCommand {
    fn name(&self) -> &str {
        &self.label
    }
    fn execute(&mut self, project: &mut Project) -> Result<bool, EditError> {
        self.state.require(CommandState::Fresh, "执行")?;
        project
            .canvas()
            .require_layer_frame(self.layer_id, self.frame_id)?;
        let (_, frame) = project.canvas().frame_location(self.frame_id)?;
        let layer = project.canvas().layer(self.layer_id)?;
        if layer.properties.locked {
            return Err(DrawError::LayerLocked(self.layer_id).into());
        }
        let cel = layer.cels.get(&self.frame_id).cloned();
        let mut targets = BTreeMap::new();
        for &(position, color) in &self.pixels {
            project.canvas().offset(position)?;
            validate_color(project, color)?;
            let key = (
                position.0 - cel.as_ref().map_or(0, |c| c.x),
                position.1 - cel.as_ref().map_or(0, |c| c.y),
            );
            targets.insert(key, color);
        }
        for (pos, color) in targets {
            let before = cel
                .as_ref()
                .and_then(|c| layer.images.get(&c.image))
                .and_then(|p| p.binary_search_by_key(&pos, |p| p.0).ok().map(|i| p[i].1));
            if color != before {
                self.changes.push((pos, before, color));
            }
        }
        if self.changes.is_empty() {
            return Ok(false);
        }
        let shared = cel.as_ref().map_or(1, |c| {
            layer
                .cels
                .values()
                .filter(|other| other.image == c.image)
                .count()
        });
        self.label = format!(
            "修改像素 · {} · 帧 {}{}",
            layer.properties.name,
            frame + 1,
            if shared > 1 {
                format!(" · 共享 {shared} 帧")
            } else {
                String::new()
            }
        );
        self.created = cel.is_none();
        self.image = Some(match cel {
            Some(c) => c.image,
            None => project.canvas_mut().allocate_image()?,
        });
        self.apply(project, true)?;
        self.pixels.clear();
        self.state = CommandState::Applied;
        Ok(true)
    }
    fn undo(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Applied, "撤销")?;
        self.apply(project, false)?;
        self.state = CommandState::Undone;
        Ok(())
    }
    fn redo(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Undone, "重做")?;
        self.apply(project, true)?;
        self.state = CommandState::Applied;
        Ok(())
    }
}

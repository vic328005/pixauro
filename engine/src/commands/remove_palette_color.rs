use crate::commands::command::{CommandState, sealed, validate_color, validate_recorded_color};
use crate::{
    Color, ColorId, Command, DrawError, EditError, ImageId, LayerId, PaletteSource, Project,
    ProjectError,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
type Reference = (LayerId, ImageId, (i32, i32));
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovePaletteColorCommand {
    id: ColorId,
    replacement: Option<ColorId>,
    previous: Option<Color>,
    source: Option<PaletteSource>,
    position: usize,
    references: Vec<Reference>,
    #[serde(skip)]
    state: CommandState,
}
impl RemovePaletteColorCommand {
    pub fn new(id: ColorId, replacement: Option<ColorId>) -> Self {
        Self {
            id,
            replacement,
            previous: None,
            source: None,
            position: 0,
            references: Vec::new(),
            state: CommandState::Fresh,
        }
    }
    fn references(p: &Project, id: ColorId) -> Vec<Reference> {
        let mut refs = Vec::new();
        for l in p.canvas().layers() {
            for (image, pixels) in l.images() {
                for &(pos, c) in pixels.iter() {
                    if c == id {
                        refs.push((l.id(), *image, pos));
                    }
                }
            }
        }
        refs.sort();
        refs
    }
    fn replace(&self, p: &mut Project, id: ColorId) -> Result<(), EditError> {
        for &(l, image, pos) in &self.references {
            let pixels = p
                .canvas()
                .layer(l)?
                .images
                .get(&image)
                .ok_or_else(|| ProjectError::InvalidData("颜色历史图像缺失".into()))?;
            if pixels.binary_search_by_key(&pos, |v| v.0).is_err() {
                return Err(ProjectError::InvalidData("颜色历史坐标缺失".into()).into());
            }
        }
        for &(l, image, pos) in &self.references {
            let pixels = Arc::make_mut(
                p.canvas_mut()
                    .layer_mut(l)?
                    .images
                    .get_mut(&image)
                    .expect("已校验"),
            );
            let i = pixels.binary_search_by_key(&pos, |v| v.0).expect("已校验");
            pixels[i].1 = id;
        }
        Ok(())
    }
    fn remove(&self, p: &mut Project) -> Result<(), EditError> {
        validate_color(p, Some(self.id))?;
        validate_color(p, self.replacement)?;
        if self.replacement == Some(self.id)
            || (!self.references.is_empty() && self.replacement.is_none())
        {
            return Err(
                ProjectError::InvalidData("请为正在使用的颜色选择另一种替代色".into()).into(),
            );
        }
        if Self::references(p, self.id) != self.references {
            return Err(ProjectError::InvalidData("颜色历史引用不匹配".into()).into());
        }
        if p.palette().entries().len() == 1 {
            return Err(DrawError::EmptyPalette.into());
        }
        if let Some(id) = self.replacement {
            self.replace(p, id)?;
        }
        p.palette_mut().remove(self.id)?;
        Ok(())
    }
}
impl sealed::Sealed for RemovePaletteColorCommand {
    fn restore(&mut self, p: &Project, applied: bool) -> Result<(), EditError> {
        validate_recorded_color(p, Some(self.id))?;
        validate_recorded_color(p, self.replacement)?;
        if self.previous.is_none()
            || self.source.is_none()
            || self.replacement == Some(self.id)
            || (!self.references.is_empty() && self.replacement.is_none())
            || self.references.windows(2).any(|p| p[0] >= p[1])
            || self.references.iter().any(|(l, i, (x, y))| {
                !p.canvas().was_allocated(*l)
                    || i.0 == 0
                    || i.0 >= p.canvas().next_animation_id
                    || x.abs_diff(0) > 12288
                    || y.abs_diff(0) > 12288
            })
        {
            return Err(ProjectError::InvalidData("颜色历史无效".into()).into());
        }
        self.state = CommandState::restored(applied);
        Ok(())
    }
}
#[typetag::serde(name = "remove_palette_color")]
impl Command for RemovePaletteColorCommand {
    fn name(&self) -> &str {
        "删除调色板颜色 · 全部帧"
    }
    fn execute(&mut self, p: &mut Project) -> Result<bool, EditError> {
        self.state.require(CommandState::Fresh, "执行")?;
        self.previous = Some(
            p.palette()
                .color(self.id)
                .ok_or(DrawError::InvalidColorId(self.id))?,
        );
        self.source = p.palette().source(self.id);
        self.position = p
            .palette()
            .entries()
            .position(|(id, _)| id == self.id)
            .expect("已校验");
        self.references = Self::references(p, self.id);
        self.remove(p)?;
        self.state = CommandState::Applied;
        Ok(true)
    }
    fn undo(&mut self, p: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Applied, "撤销")?;
        let mut next = p.clone();
        next.palette_mut().insert(
            self.position,
            self.id,
            self.previous.expect("已捕获"),
            self.source.expect("已捕获"),
        )?;
        self.replace(&mut next, self.id)?;
        *p = next;
        self.state = CommandState::Undone;
        Ok(())
    }
    fn redo(&mut self, p: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Undone, "重做")?;
        self.remove(p)?;
        self.state = CommandState::Applied;
        Ok(())
    }
}

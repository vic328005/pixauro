use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::command::{CommandState, sealed, validate_recorded_color};
use crate::{Color, ColorId, Command, EditError, PaletteSource, Project, ProjectError};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemovedColor {
    position: usize,
    id: ColorId,
    color: Color,
    source: PaletteSource,
}

/// 一次删除整个作品未引用的颜色，保留原顺序与来源供撤销恢复。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrunePaletteCommand {
    removed: Vec<RemovedColor>,
    #[serde(skip)]
    state: CommandState,
}

impl Default for PrunePaletteCommand {
    fn default() -> Self {
        Self {
            removed: Vec::new(),
            state: CommandState::Fresh,
        }
    }
}

impl PrunePaletteCommand {
    fn used_colors(project: &Project) -> HashSet<ColorId> {
        project
            .canvas()
            .layers()
            .iter()
            .flat_map(|layer| layer.images().values())
            .flat_map(|pixels| pixels.iter().map(|(_, id)| *id))
            .collect()
    }

    fn remove(&self, project: &mut Project) -> Result<(), EditError> {
        let used = Self::used_colors(project);
        let entries: Vec<_> = project.palette().entries().collect();
        let mut palette = project.palette().clone();
        for entry in &self.removed {
            if used.contains(&entry.id)
                || entries.get(entry.position) != Some(&(entry.id, entry.color))
                || palette.source(entry.id) != Some(entry.source)
            {
                return Err(ProjectError::InvalidData("精简调色板历史不匹配".into()).into());
            }
            palette.remove(entry.id)?;
        }
        *project.palette_mut() = palette;
        Ok(())
    }
}

impl sealed::Sealed for PrunePaletteCommand {
    fn restore(&mut self, project: &Project, applied: bool) -> Result<(), EditError> {
        let mut ids = HashSet::new();
        if self.removed.is_empty()
            || self
                .removed
                .windows(2)
                .any(|pair| pair[0].position >= pair[1].position)
        {
            return Err(ProjectError::InvalidData("精简调色板历史无效".into()).into());
        }
        for entry in &self.removed {
            validate_recorded_color(project, Some(entry.id))?;
            if !ids.insert(entry.id) {
                return Err(ProjectError::InvalidData("精简调色板历史颜色重复".into()).into());
            }
        }
        self.state = CommandState::restored(applied);
        Ok(())
    }
}

#[typetag::serde(name = "prune_palette")]
impl Command for PrunePaletteCommand {
    fn name(&self) -> &str {
        "精简调色板 · 删除未使用颜色"
    }

    fn execute(&mut self, project: &mut Project) -> Result<bool, EditError> {
        self.state.require(CommandState::Fresh, "执行")?;
        let used = Self::used_colors(project);
        self.removed = project
            .palette()
            .entries()
            .enumerate()
            // 空作品仍保留第一个颜色，满足调色板非空约束。
            .filter(|(position, (id, _))| !used.contains(id) && (!used.is_empty() || *position > 0))
            .map(|(position, (id, color))| RemovedColor {
                position,
                id,
                color,
                source: project.palette().source(id).expect("颜色存在"),
            })
            .collect();
        if self.removed.is_empty() {
            return Ok(false);
        }
        self.remove(project)?;
        self.state = CommandState::Applied;
        Ok(true)
    }

    fn undo(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Applied, "撤销")?;
        let mut palette = project.palette().clone();
        for entry in &self.removed {
            palette.insert(entry.position, entry.id, entry.color, entry.source)?;
        }
        *project.palette_mut() = palette;
        self.state = CommandState::Undone;
        Ok(())
    }

    fn redo(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Undone, "重做")?;
        self.remove(project)?;
        self.state = CommandState::Applied;
        Ok(())
    }
}

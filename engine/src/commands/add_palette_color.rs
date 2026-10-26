use serde::{Deserialize, Serialize};

use crate::commands::command::{CommandState, sealed, validate_recorded_color};
use crate::{Color, ColorId, Command, EditError, PaletteSource, Project, ProjectError};

/// 追加一个颜色。首次执行分配 ID，undo/redo 始终沿用这个 ID。
/// 分配计数不回退，撤销后创建的新分支不会与旧命令引用发生碰撞。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddPaletteColorCommand {
    color: Color,
    id: Option<ColorId>,
    position: usize,
    #[serde(skip)]
    state: CommandState,
}

impl AddPaletteColorCommand {
    pub fn new(color: Color) -> Self {
        Self {
            color,
            id: None,
            position: 0,
            state: CommandState::Fresh,
        }
    }
}

impl sealed::Sealed for AddPaletteColorCommand {
    fn restore(&mut self, project: &Project, applied: bool) -> Result<(), EditError> {
        let id = self
            .id
            .ok_or_else(|| ProjectError::InvalidData("新增颜色命令缺少 ID".into()))?;
        validate_recorded_color(project, Some(id))?;
        self.state = CommandState::restored(applied);
        Ok(())
    }
}

#[typetag::serde(name = "add_palette_color")]
impl Command for AddPaletteColorCommand {
    fn name(&self) -> &str {
        "添加调色板颜色"
    }

    fn execute(&mut self, project: &mut Project) -> Result<bool, EditError> {
        self.state.require(CommandState::Fresh, "执行")?;
        self.position = project.palette().entries().len();
        self.id = Some(
            project
                .palette_mut()
                .append(self.color, PaletteSource::Custom)?,
        );
        self.state = CommandState::Applied;
        Ok(true)
    }

    fn undo(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Applied, "撤销")?;
        let id = self.id.expect("已执行命令有 ID");
        // 正常历史顺序中，后续使用该颜色的命令已先被撤销。
        // 即便文件被手动改坏，也不能删出悬空的像素引用。
        if project.canvas().layers().iter().any(|layer| {
            layer
                .images()
                .values()
                .any(|pixels| pixels.iter().any(|(_, color)| *color == id))
        }) {
            return Err(ProjectError::InvalidData("颜色仍被像素使用，无法撤销添加".into()).into());
        }
        project.palette_mut().remove(id)?;
        self.state = CommandState::Undone;
        Ok(())
    }

    fn redo(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Undone, "重做")?;
        project.palette_mut().insert(
            self.position,
            self.id.expect("已执行命令有 ID"),
            self.color,
            PaletteSource::Custom,
        )?;
        self.state = CommandState::Applied;
        Ok(())
    }
}

use serde::{Deserialize, Serialize};

use crate::commands::command::{CommandState, sealed};
use crate::{Color, ColorId, Command, DrawError, EditError, Project};

/// 调色板条目的修改与恢复都属于同一条命令，不触碰引用它的像素颜色引用。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetPaletteColorCommand {
    id: ColorId,
    color: Color,
    previous: Option<Color>,
    // 文件不保存运行期状态，加载时由历史游标恢复。
    #[serde(skip)]
    state: CommandState,
}
impl SetPaletteColorCommand {
    pub fn new(id: ColorId, color: Color) -> Self {
        Self {
            id,
            color,
            previous: None,
            state: CommandState::Fresh,
        }
    }
}
impl sealed::Sealed for SetPaletteColorCommand {
    fn restore(&mut self, project: &Project, applied: bool) -> Result<(), EditError> {
        crate::commands::command::validate_recorded_color(project, Some(self.id))?;
        // 已入栈的改色命令必须有旧颜色，否则 undo 无法恢复。
        if self.previous.is_none() {
            return Err(crate::ProjectError::InvalidData("改色命令缺少撤销颜色".into()).into());
        }
        self.state = CommandState::restored(applied);
        Ok(())
    }
}
#[typetag::serde(name = "set_palette_color")]
impl Command for SetPaletteColorCommand {
    fn name(&self) -> &str {
        "修改调色板颜色"
    }

    fn execute(&mut self, project: &mut Project) -> Result<bool, EditError> {
        self.state.require(CommandState::Fresh, "执行")?;
        let previous = project
            .palette()
            .color(self.id)
            .ok_or(DrawError::InvalidColorId(self.id))?;
        self.previous = Some(previous);
        project.restore_color(self.id, self.color)?;
        self.state = CommandState::Applied;
        Ok(previous != self.color)
    }

    fn undo(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Applied, "撤销")?;
        project.restore_color(self.id, self.previous.expect("成功执行时已捕获旧颜色"))?;
        self.state = CommandState::Undone;
        Ok(())
    }

    fn redo(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Undone, "重做")?;
        project.restore_color(self.id, self.color)?;
        self.state = CommandState::Applied;
        Ok(())
    }
}

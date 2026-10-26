//! 动画命令的执行、撤销、重做和历史恢复。
mod edit;
use super::{
    canvas_change::CanvasChange,
    command::{CommandState, sealed},
    invalid,
};
use crate::{Command, EditError, Project};
pub use edit::AnimationEdit;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationCommand {
    edit: AnimationEdit,
    change: Option<CanvasChange>,
    label: String,
    #[serde(skip)]
    state: CommandState,
}
impl AnimationCommand {
    pub fn new(edit: AnimationEdit) -> Self {
        Self {
            edit,
            change: None,
            label: String::new(),
            state: CommandState::Fresh,
        }
    }
}
impl sealed::Sealed for AnimationCommand {
    fn restore(&mut self, p: &Project, applied: bool) -> Result<(), EditError> {
        self.change
            .as_ref()
            .ok_or_else(|| invalid("动画命令缺少历史"))?
            .validate(p)?;
        self.state = CommandState::restored(applied);
        Ok(())
    }
}
#[typetag::serde(name = "animation")]
impl Command for AnimationCommand {
    fn name(&self) -> &str {
        &self.label
    }
    fn execute(&mut self, p: &mut Project) -> Result<bool, EditError> {
        self.state.require(CommandState::Fresh, "执行")?;
        let mut next = p.canvas().clone();
        self.label = self.edit.apply(&mut next)?;
        next.validate(p.palette(), false)?;
        if &next == p.canvas() {
            return Ok(false);
        }
        self.change = Some(CanvasChange::new(p.canvas(), &next));
        *p.canvas_mut() = next;
        self.state = CommandState::Applied;
        Ok(true)
    }
    fn undo(&mut self, p: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Applied, "撤销")?;
        let palette = p.palette().clone();
        self.change
            .as_ref()
            .ok_or_else(|| invalid("缺少历史"))?
            .apply(p.canvas_mut(), &palette, false)?;
        self.state = CommandState::Undone;
        Ok(())
    }
    fn redo(&mut self, p: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Undone, "重做")?;
        let palette = p.palette().clone();
        self.change
            .as_ref()
            .ok_or_else(|| invalid("缺少历史"))?
            .apply(p.canvas_mut(), &palette, true)?;
        self.state = CommandState::Applied;
        Ok(())
    }
}

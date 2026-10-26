//! 操作历史只保存执行过的命令对象和游标，不保存统一格式的变更结果。

use serde::{Deserialize, Serialize};

use crate::{Command, EditError, Project};

/// entries[..position] 已执行，entries[position..] 已撤销、可以重做。
/// 操作栈仅在引擎内部使用；外部通过 Engine 操作，HistoryView 只提供展示摘要。
#[derive(Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommandHistory {
    entries: Vec<Box<dyn Command>>,
    position: usize,
    // 仅供当前实例识别操作栈是否追加/替换过命令，不进入 JSON。
    // 只比较画布或 entries.len() 无法识别“撤销后画出相同内容”的新分支。
    #[serde(skip)]
    generation: u64,
}

/// 面向显示层的只读历史视图，只提供操作名称与当前步骤位置。
/// 借用现有历史，不复制命令及撤销数据，也不允许外部执行或修改栈中的命令。
pub struct HistoryView<'a> {
    history: &'a CommandHistory,
}

impl HistoryView<'_> {
    /// 按操作顺序读取名称；迭代器长度是总命令数，可用于滚动列表。
    pub fn entries(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.history.entries.iter().map(|command| command.name())
    }

    /// 已执行的命令数。0 表示历史起点，之后的命令保留在栈中等待重做。
    pub fn position(&self) -> usize {
        self.history.position
    }
}

impl CommandHistory {
    pub(crate) fn view(&self) -> HistoryView<'_> {
        HistoryView { history: self }
    }

    /// 当前命令列表的版本与游标共同作为保存标记，仅在同一 Engine 内比较。
    /// 命令追加会改变版本，撤销/重做只移动游标。
    pub(crate) fn mark(&self) -> (u64, usize) {
        (self.generation, self.position)
    }

    /// typetag 已恢复具体命令类型。这里只检查游标及每条命令的基本有效性，
    /// 再根据游标设置 Applied/Undone；不复制作品、不执行或重演任何命令。
    pub(crate) fn restore(&mut self, project: &Project) -> Result<(), EditError> {
        if self.position > self.entries.len() {
            return Err(crate::ProjectError::InvalidData("历史游标超出操作栈范围".into()).into());
        }
        for (index, command) in self.entries.iter_mut().enumerate() {
            command.restore(project, index < self.position)?;
        }
        Ok(())
    }

    pub(crate) fn can_undo(&self) -> bool {
        self.position > 0
    }
    pub(crate) fn can_redo(&self) -> bool {
        self.position < self.entries.len()
    }

    pub(crate) fn push_executed(&mut self, command: Box<dyn Command>) {
        // 新命令已经执行成功，才丢弃游标后的旧分支；失败/无变化命令不会到这里。
        self.entries.truncate(self.position);
        self.entries.push(command);
        self.position += 1;
        self.generation = self.generation.wrapping_add(1);
    }

    pub(crate) fn undo(&mut self, project: &mut Project) -> Result<bool, EditError> {
        if !self.can_undo() {
            return Ok(false);
        }
        self.entries[self.position - 1].undo(project)?;
        self.position -= 1;
        Ok(true)
    }

    pub(crate) fn redo(&mut self, project: &mut Project) -> Result<bool, EditError> {
        if !self.can_redo() {
            return Ok(false);
        }
        self.entries[self.position].redo(project)?;
        self.position += 1;
        Ok(true)
    }
}

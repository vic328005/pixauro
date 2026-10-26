//! Engine 是引擎对外的编辑入口，一个实例管理一个项目及其撤销历史。
//! 路径、保存提示、画笔选择和鼠标事件仍由调用方管理。

mod history;
use crate::{Command, EditError, Palette, Project, ProjectError, ProjectOptions};
pub(crate) use history::CommandHistory;
pub use history::HistoryView;

/// 外部需要的编辑状态快照。修改这些字段不会影响 Engine 本身。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineStatus {
    /// 相对新建/加载/上次成功保存，项目或历史是否发生变化。
    pub modified: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// 同一实例的内容版本，供显示缓存使用；不是文件版本或保存标记。
    pub revision: u64,
}

/// 保存基准只留在引擎内，Editor 无需复制整份作品或理解历史结构。
/// 作品修改全部通过命令，栈版本和游标足以识别已提交的修改；
/// 改名独立于命令，所以另外记录名称。工具的临时预览不属于项目内容。
#[derive(Debug)]
struct SavedState {
    name: String,
    history: (u64, usize),
}

#[derive(Debug)]
pub struct Engine {
    pub(crate) project: Project,
    pub(crate) history: CommandHistory,
    // 用于显示层判断缓存是否过期，不表示保存状态；撤销也会递增。
    revision: u64,
    saved: SavedState,
}

impl Engine {
    /// 创建项目及空操作栈；所有配置在创建前校验，失败不产生半有效实例。
    /// 新建空白项目以初始内容为未修改基准，保留原有编辑器行为。
    pub fn new(options: ProjectOptions) -> Result<Self, ProjectError> {
        let project = Project::new(
            options.name,
            options.width,
            options.height,
            Palette::new(options.palette)?,
        )?;
        Ok(Self::from_parts(project, CommandHistory::default()))
    }

    /// 新建与加载共用内部装配入口，加载会传入已经校验的历史。
    pub(crate) fn from_parts(project: Project, history: CommandHistory) -> Self {
        let saved = SavedState {
            name: project.name().to_owned(),
            history: history.mark(),
        };
        Self {
            project,
            history,
            revision: 0,
            saved,
        }
    }

    /// 文件成功落盘之后才能更新基准；保存失败时保留原来的 modified 状态。
    pub(crate) fn mark_saved(&mut self) {
        self.saved = SavedState {
            name: self.project.name().to_owned(),
            history: self.history.mark(),
        };
    }

    /// 仅暴露只读引用，防止外部修改项目导致历史失效。
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// 为历史面板提供只读展示数据；撤销和重做仍通过 Engine 调度。
    pub fn history(&self) -> HistoryView<'_> {
        self.history.view()
    }

    /// 项目管理接口：只更新名称，不入操作栈，不移动游标，也不清除重做分支。
    /// 名称与绘图命令的恢复数据无关，因此不影响已有的撤销记录。
    pub fn rename(&mut self, name: impl Into<String>) -> Result<bool, ProjectError> {
        let changed = self.project.rename(name)?;
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(changed)
    }

    /// 将日常编辑状态汇总为一个快照，无需由外部根据历史或保存基准自行计算。
    /// 同一栈撤销再重做到保存位置会恢复干净状态；新增重做分支仍属于修改。
    pub fn status(&self) -> EngineStatus {
        EngineStatus {
            modified: self.project.name() != self.saved.name
                || self.history.mark() != self.saved.history,
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            revision: self.revision,
        }
    }

    /// 一次提交一个完整操作；引擎只管理正式项目内容，不持有工具的临时状态。
    pub fn execute(&mut self, command: impl Command + 'static) -> Result<bool, EditError> {
        let mut command: Box<dyn Command> = Box::new(command);
        let mut next = self.project.clone();
        if !command.execute(&mut next)? {
            return Ok(false);
        }
        next.canvas().validate(next.palette(), false)?;
        self.project = next;
        self.history.push_executed(command);
        self.revision = self.revision.wrapping_add(1);
        Ok(true)
    }

    pub fn undo(&mut self) -> Result<bool, EditError> {
        let changed = self.history.undo(&mut self.project)?;
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(changed)
    }

    pub fn redo(&mut self) -> Result<bool, EditError> {
        let changed = self.history.redo(&mut self.project)?;
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(changed)
    }
}

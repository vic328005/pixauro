//! 可撤销命令的共同协议。命令同时持有操作参数、撤销数据和执行状态。
//! Engine 将命令本身存入操作栈，不再把它转换成统一的变更记录。

use std::fmt::Debug;

use crate::{ColorId, EditError, Project};

// 当前只允许引擎内实现命令，确保“失败不修改项目”和 undo/redo 的约定成立。
// 调用方可以构造和提交命令；扩展新的内置操作时，在 commands 中实现本协议。
pub(crate) mod sealed {
    pub trait Sealed {
        /// 加载历史时检查本命令的基本有效性，再按游标恢复生命周期。
        /// 此过程不执行命令、不修改作品，也不验证历史结果是否与画面一致。
        fn restore(
            &mut self,
            project: &crate::Project,
            applied: bool,
        ) -> Result<(), crate::EditError>;
    }
}

/// 每种操作将执行、撤销、重做放在同一个实现中。
///
/// execute 只运行一次，负责校验参数并捕获旧值；返回 false 表示没有实际修改。
/// undo 恢复捕获的旧值，redo 恢复首次执行的结果，不能重新捕获旧值。
/// 生命周期方法由 Engine 调用；调用方应构造新命令后直接交给 Engine::execute。
/// 执行后的命令必须与原项目及历史顺序绑定，不能拿去对其他项目撤销或重做。
/// 命令由 Engine 接管后不会再向外提供可变引用，避免操作栈与项目内容脱节。
/// typetag 根据各实现的 name 标签注册类型，直接读写 Box<dyn Command>。
/// 新增命令只需添加对应宏与恢复检查，不维护额外的记录枚举。
#[typetag::serde(tag = "type", content = "data")]
pub trait Command: sealed::Sealed + Debug + Send {
    fn name(&self) -> &str;
    fn execute(&mut self, project: &mut Project) -> Result<bool, EditError>;
    fn undo(&mut self, project: &mut Project) -> Result<(), EditError>;
    fn redo(&mut self, project: &mut Project) -> Result<(), EditError>;
}

/// 防止把已经执行过的命令交给另一份项目，或重复 undo/redo。
/// 参数错误不改变状态，命令的执行与恢复由 Engine 按操作栈顺序调度。
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) enum CommandState {
    // 直接反序列化得到的命令不能立即执行或撤销，需先经过历史加载入口。
    // 新建命令由构造函数显式标记为 Fresh，避免沿用文件中的旧撤销数据执行。
    #[default]
    Unvalidated,
    Fresh,
    Applied,
    Undone,
}

impl CommandState {
    pub(crate) fn restored(applied: bool) -> Self {
        if applied { Self::Applied } else { Self::Undone }
    }

    pub fn require(&self, expected: Self, operation: &'static str) -> Result<(), EditError> {
        if *self == expected {
            Ok(())
        } else {
            Err(EditError::InvalidCommandState(operation))
        }
    }
}

/// 执行前检查颜色 ID 当前存在，防止写入悬空的像素引用。
pub(crate) fn validate_color(project: &Project, index: Option<ColorId>) -> Result<(), EditError> {
    if let Some(index) = index
        && project.palette().color(index).is_none()
    {
        return Err(crate::DrawError::InvalidColorId(index).into());
    }
    Ok(())
}

/// 加载不重放历史：已删除、或等待 redo 添加的颜色仍可以出现在命令里。
pub(crate) fn validate_recorded_color(
    project: &Project,
    id: Option<ColorId>,
) -> Result<(), EditError> {
    if let Some(id) = id
        && !project.palette().was_allocated(id)
    {
        return Err(crate::DrawError::InvalidColorId(id).into());
    }
    Ok(())
}

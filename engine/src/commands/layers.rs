//! 图层管理命令：新增、复制、删除、修改属性与调整顺序。
//!
//! 本模块把一次编辑分成两部分：Action 表达调用方想做什么，Change 保存首次
//! 执行时确定的结果及撤销所需数据。execute 根据当时的作品捕获 Change，后续
//! undo/redo 只反向或正向应用它，不重新读取源图层来生成副本，也不重新分配 ID。
//!
//! 图层在各自动画内按从底到顶排列，公开位置 0 是该动画最底层；LayerId 不变。
//! 历史中的位置使用底层扁平数组下标，以精确恢复原始结构。
//! 这些操作只修改正式作品，当前选中层、笔画预览和界面状态由调用方管理。
//! 命令连同撤销数据一起保存到项目文件，运行期生命周期由历史游标恢复。

use crate::commands::command::{CommandState, sealed};
use crate::{
    AnimationId, Canvas, Command, DrawError, EditError, Layer, LayerId, LayerProperties, Project,
    ProjectError,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 调用方提交的编辑意图；构造命令时尚未读取或修改项目。
/// 执行成功后仍保留它，用于区分历史面板中的“新增”和“复制”等操作名称。
enum Action {
    /// 新增与复制共用插入流程：None 创建空层，Some(id) 复制指定层。
    Add {
        name: String,
        animation: Option<AnimationId>,
        source: Option<LayerId>,
        above: Option<LayerId>,
    },
    /// 删除指定身份的图层，执行时再捕获完整内容及其当前位置。
    Remove { id: LayerId },
    /// 整体替换名称、可见性、锁定状态和不透明度，不改变像素或图层身份。
    Properties {
        id: LayerId,
        properties: LayerProperties,
    },
    /// position 是移动完成后的最终下标，不是删除源元素前的插入缝隙。
    Move { id: LayerId, position: usize },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 首次执行捕获的可逆变更，后续撤销、重做均以此为准。
/// 插入和删除需要保留完整图层快照；属性和排序只保存各自的前后值，避免复制像素。
enum Change {
    /// 正向插入、反向删除。快照包含首次分配的 ID，重做时恢复相同身份。
    Insert { layer: Layer, position: usize },
    /// 正向删除、反向插入。Layer 保留原属性和稀疏像素，position 保留原顺序。
    Remove { layer: Layer, position: usize },
    /// 只替换属性；before 用于撤销，after 用于首次执行和重做。
    Properties {
        id: LayerId,
        before: LayerProperties,
        after: LayerProperties,
    },
    /// 同时记录身份与前后位置，应用时检查源位置确实仍对应这一层。
    Move {
        id: LayerId,
        before: usize,
        after: usize,
    },
}

/// 一次可撤销的图层编辑；通过构造函数创建，再交给 Engine::execute 调度。
///
/// 有实际变化时占一条历史记录；属性未变或原地移动返回 false，不新增历史，
/// 也不会清除已有的重做分支。所有位置均从底层 0 开始。
/// 每个动画至少保留一层，新增和恢复图层也受 Canvas 的层数及总像素容量限制。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerCommand {
    // 原始请求，决定首次执行的行为以及历史中展示的名称。
    action: Action,
    // Fresh 时为 None；实际编辑成功后保存变更，随命令序列化。
    change: Option<Change>,
    label: String,
    // 不序列化运行期状态。反序列化后先为 Unvalidated，由 restore 检查后恢复。
    #[serde(skip)]
    state: CommandState,
}

/// 将位置、历史结构等业务错误统一包装为引擎编辑错误。
fn invalid(message: &str) -> EditError {
    ProjectError::InvalidData(message.into()).into()
}

impl LayerCommand {
    /// 只创建待执行命令；参数依赖的项目状态在 execute 中统一校验。
    fn new(action: Action) -> Self {
        Self {
            action,
            change: None,
            label: String::new(),
            state: CommandState::Fresh,
        }
    }
    /// 在指定动画最顶端新增可见、完全不透明的空图层。
    /// 名称在执行时去掉首尾空白并校验，此时才分配新 LayerId。
    pub fn add(animation: AnimationId, name: impl Into<String>) -> Self {
        Self::new(Action::Add {
            name: name.into(),
            animation: Some(animation),
            source: None,
            above: None,
        })
    }
    /// 在指定图层正上方创建空层，作为一条历史记录。
    pub fn add_above(id: LayerId, name: impl Into<String>) -> Self {
        Self::new(Action::Add {
            name: name.into(),
            animation: None,
            source: None,
            above: Some(id),
        })
    }
    /// 把指定层的像素、可见性、锁定状态和不透明度复制到它的正上方，并使用给定名称。
    /// 副本拥有新的 LayerId；首次执行后源层再发生变化，也不会改变已捕获的副本。
    pub fn duplicate(id: LayerId, name: impl Into<String>) -> Self {
        Self::new(Action::Add {
            name: name.into(),
            animation: None,
            source: Some(id),
            above: Some(id),
        })
    }
    /// 删除指定层；不能删除最后一层。撤销时恢复原身份、内容和位置。
    pub fn remove(id: LayerId) -> Self {
        Self::new(Action::Remove { id })
    }
    /// 一次替换全部图层属性，调用方如只改一项，应先读取并保留其他属性。
    /// 名称规范化后与原属性相同时，不产生历史记录。
    pub fn set_properties(id: LayerId, properties: LayerProperties) -> Self {
        Self::new(Action::Properties { id, properties })
    }
    /// 移动到所属动画内的最终位置，合法范围为 0..该动画图层数；原地移动不产生历史。
    /// 例如 [A, B, C] 中把 A 移到 2，结果是 [B, C, A]。
    pub fn move_to(id: LayerId, position: usize) -> Self {
        Self::new(Action::Move { id, position })
    }

    /// 应用已捕获的变更：forward=true 用于首次执行及重做，false 用于撤销。
    /// 每个分支先完成可能失败的校验，再修改作品；调用方只在成功后推进命令状态。
    /// 本函数不捕获旧值、不分配 ID，也不自行移动历史游标。
    fn apply(change: &Change, project: &mut Project, forward: bool) -> Result<(), EditError> {
        let mut next = project.clone();
        Self::apply_inner(change, &mut next, forward)?;
        next.canvas().validate(next.palette(), false)?;
        *project = next;
        Ok(())
    }
    fn apply_inner(change: &Change, project: &mut Project, forward: bool) -> Result<(), EditError> {
        match change {
            Change::Insert { layer, position } | Change::Remove { layer, position } => {
                // 同一套检查覆盖四种情况：执行/重做新增与撤销删除都插入；
                // 执行/重做删除与撤销新增都删除。
                let insert = matches!(change, Change::Insert { .. }) == forward;
                let canvas = project.canvas();
                if insert {
                    // 插入位置可以等于当前长度（追加）；ID 必须已分配且当前不存在。
                    // 撤销不会回退分配器，因此历史快照可以恢复原 ID，而无需再分配。
                    canvas.check_capacity(canvas.layers().len() + 1)?;
                    if *position > canvas.layers().len()
                        || canvas.layer(layer.id).is_ok()
                        || !canvas.was_allocated(layer.id)
                    {
                        return Err(invalid("无法恢复图层：位置或 ID 无效"));
                    }
                    // false 表示正在恢复真实图层：所有颜色引用此刻必须存在。
                    // 快照保存稀疏图像与 cel 引用，恢复时复用未修改的像素集合。
                    layer.validate(project.palette(), false)?;
                    let restored = layer.clone();
                    project.canvas_mut().layers.insert(*position, restored);
                } else {
                    // 即使是撤销新增，也必须遵守至少保留一层的约束。
                    // 同时核验位置与 ID，避免损坏的历史删掉另一层。
                    canvas.check_capacity(canvas.layers().len().saturating_sub(1))?;
                    if canvas.animation_layers(layer.animation).count() <= 1 {
                        return Err(invalid("每个动画至少保留一个图层"));
                    }
                    if canvas
                        .layers()
                        .get(*position)
                        .is_none_or(|l| l.id() != layer.id)
                    {
                        return Err(invalid("删除图层的位置与历史不一致"));
                    }
                    project.canvas_mut().layers.remove(*position);
                }
            }
            Change::Properties { id, before, after } => {
                // 属性通过稳定 ID 定位，不受其他图层位置变化影响。
                let properties = if forward { after } else { before };
                properties.validate()?;
                project.canvas_mut().layer_mut(*id)?.properties = properties.clone();
            }
            Change::Move { id, before, after } => {
                let (from, to) = if forward {
                    (*before, *after)
                } else {
                    (*after, *before)
                };
                let canvas = project.canvas_mut();
                if to >= canvas.layers.len()
                    || canvas.layers.get(from).is_none_or(|l| l.id() != *id)
                {
                    return Err(invalid("移动图层的位置与历史不一致"));
                }
                // to 已经是最终下标，移除源元素后直接插入；向上移动无需再减一。
                let layer = canvas.layers.remove(from);
                canvas.layers.insert(to, layer);
            }
        }
        Ok(())
    }
}

impl sealed::Sealed for LayerCommand {
    /// 加载项目时只检查历史数据的结构与取值范围，不重演历史，也不修改作品。
    /// applied 来自历史游标：游标前的命令可撤销，游标后的命令可重做。
    fn restore(&mut self, project: &Project, applied: bool) -> Result<(), EditError> {
        let canvas = project.canvas();
        let change = self
            .change
            .as_ref()
            .ok_or_else(|| invalid("图层命令缺少撤销数据"))?;
        let id = match change {
            Change::Insert { layer, position } | Change::Remove { layer, position } => {
                // 历史位置可能超过当前层数（例如该层已删除），这里只检查全局上限。
                if *position >= Canvas::MAX_LAYERS {
                    return Err(invalid("图层历史位置无效"));
                }
                // true 允许快照引用已删除或尚待重做添加的颜色，但 ID 必须曾分配。
                // 到实际 undo/redo 恢复图层时，apply 才要求这些颜色当前存在。
                layer.validate(project.palette(), true)?;
                layer.id
            }
            Change::Properties { id, before, after } => {
                before.validate()?;
                after.validate()?;
                *id
            }
            Change::Move { id, before, after } => {
                if *before >= Canvas::MAX_LAYERS || *after >= Canvas::MAX_LAYERS {
                    return Err(invalid("图层历史位置无效"));
                }
                *id
            }
        };
        // 层本身也可能已被删除或等待重做新增，不能要求它此刻在 layers 中。
        // 检查分配范围即可；实际操作时再验证存在性与位置。
        if !canvas.was_allocated(id) {
            return Err(DrawError::InvalidLayerId(id).into());
        }
        // action 只保留用户意图，重做始终使用首次捕获的 change。
        self.state = CommandState::restored(applied);
        Ok(())
    }
}

// 项目历史使用稳定的类型标签 "layer" 识别本命令，运行期状态不写入文件。
#[typetag::serde(name = "layer")]
impl Command for LayerCommand {
    /// 历史面板显示的是用户操作名称，因此新增和复制虽然共享插入逻辑仍分开展示。
    fn name(&self) -> &str {
        if !self.label.is_empty() {
            return &self.label;
        }
        match &self.action {
            Action::Add {
                source: Some(_), ..
            } => "复制图层",
            Action::Add { .. } => "新增图层",
            Action::Remove { .. } => "删除图层",
            Action::Properties { .. } => "修改图层属性",
            Action::Move { .. } => "调整图层顺序",
        }
    }
    /// 首次执行：校验请求、捕获撤销数据，再应用变更。一个命令只能执行一次。
    fn execute(&mut self, project: &mut Project) -> Result<bool, EditError> {
        self.state.require(CommandState::Fresh, "执行")?;
        let canvas = project.canvas();
        let label = match &self.action {
            Action::Add { name, source, .. } => format!(
                "{} · {}",
                if source.is_some() {
                    "复制图层"
                } else {
                    "新增图层"
                },
                name.trim()
            ),
            Action::Remove { id } => format!("删除图层 · {}", canvas.layer(*id)?.properties().name),
            Action::Move { id, .. } => {
                format!("移动图层 · {}", canvas.layer(*id)?.properties().name)
            }
            Action::Properties { id, properties } => {
                let before = canvas.layer(*id)?.properties();
                let operation = if before.name != properties.name.trim() {
                    "重命名图层"
                } else if before.visible != properties.visible {
                    if properties.visible {
                        "显示图层"
                    } else {
                        "隐藏图层"
                    }
                } else if before.locked != properties.locked {
                    if properties.locked {
                        "锁定图层"
                    } else {
                        "解锁图层"
                    }
                } else if before.opacity != properties.opacity {
                    "不透明度"
                } else {
                    "修改图层属性"
                };
                format!("{operation} · {}", before.name)
            }
        };
        let change = match &self.action {
            Action::Add {
                name,
                animation,
                source,
                above,
            } => {
                canvas.check_capacity(canvas.layers().len() + 1)?;
                let owner = match above {
                    Some(id) => canvas.layer(*id)?.animation,
                    None => animation.ok_or_else(|| invalid("新图层必须指定所属动画"))?,
                };
                canvas.animation(owner)?;
                // 指定锚点时插在它上方；未指定则追加到顶部。先确认锚点存在。
                let position = match above {
                    Some(id) => {
                        canvas
                            .layers()
                            .iter()
                            .position(|l| l.id() == *id)
                            .ok_or(DrawError::InvalidLayerId(*id))?
                            + 1
                    }
                    None => canvas.layers().len(),
                };
                let mut layer = match source {
                    Some(id) => canvas.layer(*id)?.clone(),
                    // 0 只是尚未分配身份的内部占位，不会写入正式图层或历史。
                    None => Layer {
                        id: LayerId(0),
                        animation: owner,
                        properties: LayerProperties {
                            name: name.clone(),
                            visible: true,
                            locked: false,
                            opacity: 255,
                        },
                        auto_link: false,
                        cels: Default::default(),
                        images: Default::default(),
                    },
                };
                layer.properties.name = name.trim().to_owned();
                layer.properties.validate()?;
                // 容量、源层和名称已校验，最后才分配新 ID。
                // 后续重做使用 Change 中的该 ID；撤销不回退分配器，避免身份复用。
                layer.id = project.canvas_mut().allocate_id()?;
                if source.is_some() {
                    let mut mapping = std::collections::BTreeMap::new();
                    let mut images = std::collections::BTreeMap::new();
                    for (old, pixels) in layer.images {
                        let new = project.canvas_mut().allocate_image()?;
                        mapping.insert(old, new);
                        images.insert(new, pixels);
                    }
                    for cel in layer.cels.values_mut() {
                        cel.image = mapping[&cel.image];
                    }
                    layer.images = images;
                }
                Change::Insert { layer, position }
            }
            Action::Remove { id } => {
                // 锁定只约束新编辑；撤销/重做仍按既有历史恢复图层。
                if canvas.layer(*id)?.properties().locked {
                    return Err(DrawError::LayerLocked(*id).into());
                }
                // 删除前捕获完整快照及顺序，撤销不能只恢复一个空壳图层。
                let layer = canvas.layer(*id)?.clone();
                let position = canvas
                    .layers()
                    .iter()
                    .position(|l| l.id() == *id)
                    .expect("已校验图层");
                Change::Remove { layer, position }
            }
            Action::Properties { id, properties } => {
                let before = canvas.layer(*id)?.properties().clone();
                let mut after = properties.clone();
                after.name = after.name.trim().to_owned();
                after.validate()?;
                // 比较规范化后的属性；仅增删名称首尾空格也视为没有实际变化。
                if before == after {
                    return Ok(false);
                }
                Change::Properties {
                    id: *id,
                    before,
                    after,
                }
            }
            Action::Move { id, position } => {
                let owner = canvas.layer(*id)?.animation;
                let after = canvas
                    .layers()
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.animation == owner)
                    .nth(*position)
                    .map(|(index, _)| index)
                    .ok_or_else(|| invalid("目标图层位置越界"))?;
                let before = canvas
                    .layers()
                    .iter()
                    .position(|l| l.id() == *id)
                    .ok_or(DrawError::InvalidLayerId(*id))?;
                // 无变化时不调用 apply，也不保存 Change；Engine 不会将其入栈。
                if before == after {
                    return Ok(false);
                }
                Change::Move {
                    id: *id,
                    before,
                    after,
                }
            }
        };
        // 先应用成功，再记录撤销数据并标记 Applied；Engine 随后接管历史入栈。
        Self::apply(&change, project, true)?;
        self.label = label;
        self.change = Some(change);
        self.state = CommandState::Applied;
        Ok(true)
    }
    /// 反向应用首次捕获的 Change；失败时保留 Applied，历史游标也不会前移或后退。
    fn undo(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Applied, "撤销")?;
        Self::apply(self.change.as_ref().expect("已捕获变更"), project, false)?;
        self.state = CommandState::Undone;
        Ok(())
    }
    /// 正向应用原 Change，不重新执行 Action，因此不重新复制源层或分配 ID。
    /// 成功后才恢复 Applied，确保失败仍可保留在当前历史位置。
    fn redo(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Undone, "重做")?;
        Self::apply(self.change.as_ref().expect("已捕获变更"), project, true)?;
        self.state = CommandState::Applied;
        Ok(())
    }
}

//! 项目是本地保存和打开的单位，目前恰好包含一幅作品。
//! 只负责作品身份、名称和内容查询；文件与历史封装由 io 和 engine 管理。

use crate::{Canvas, Color, ColorId, DrawError, FrameId, Palette, ProjectError};
use uuid::Uuid;

/// 新建项目需要的配置。默认是“未命名项目”、24×24 画布和内置 8 色。
/// 配置只是输入数据；名称、尺寸和调色板由 Engine::new 统一校验。
#[derive(Clone, Debug)]
pub struct ProjectOptions {
    pub name: String,
    pub width: u16,
    pub height: u16,
    pub palette: Vec<Color>,
}

impl Default for ProjectOptions {
    fn default() -> Self {
        Self {
            name: "未命名项目".into(),
            width: 24,
            height: 24,
            palette: Palette::default()
                .entries()
                .map(|(_, color)| color)
                .collect(),
        }
    }
}

/// 项目直接持有名称、调色板和画布；作品编辑通过 Engine 提交 Command，改名属于独立的项目管理操作。
/// 项目内容不记录本地路径、选中画笔、鼠标位置或未保存标志。
/// 历史由 Engine 管理并与项目一起保存；路径和工具状态由桌面管理，不进入项目 JSON。
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    // 持久身份独立于名称与路径；克隆、改名、保存和重新加载均保留此值。
    id: Uuid,
    name: String,
    palette: Palette,
    canvas: Canvas,
}

impl Project {
    /// 仅由 Engine 创建；外部使用 ProjectOptions 配置新项目。
    pub(crate) fn new(
        name: impl Into<String>,
        width: u16,
        height: u16,
        palette: Palette,
    ) -> Result<Self, ProjectError> {
        Ok(Self {
            id: Uuid::new_v4(),
            name: Self::validate_name(&name.into())?,
            palette,
            canvas: Canvas::new(width, height)?,
        })
    }

    /// 新建项目生成随机 UUID v4；外部只读，避免编辑过程中改变项目身份。
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub(crate) fn duplicate(&self, name: &str) -> Result<Self, ProjectError> {
        let mut project = self.clone();
        project.id = Uuid::new_v4();
        project.rename(name)?;
        Ok(project)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// 可见图层按从底到顶进行普通 source-over 合成，最终量化为 RGBA8。
    pub fn color_at(&self, position: (i32, i32)) -> Result<Color, DrawError> {
        let offset = self.canvas.offset(position)?;
        Ok(self.composite_at(offset))
    }

    pub(crate) fn composite_at(&self, offset: usize) -> Color {
        self.composite_frame_at(self.canvas.frames()[0].id, offset)
    }

    pub(crate) fn canvas_mut(&mut self) -> &mut Canvas {
        &mut self.canvas
    }

    /// 创建与改名共用同一套名称校验规则。
    fn validate_name(name: &str) -> Result<String, ProjectError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 128 || name.chars().any(char::is_control) {
            return Err(ProjectError::InvalidData(
                "项目名称须为 1～128 个字符，且不能包含控制字符".into(),
            ));
        }
        Ok(name.to_owned())
    }

    pub fn color_at_frame(
        &self,
        frame: FrameId,
        position: (i32, i32),
    ) -> Result<Color, ProjectError> {
        self.canvas.frame(frame)?;
        let offset = self.canvas.offset(position)?;
        Ok(self.composite_frame_at(frame, offset))
    }
    pub(crate) fn composite_frame_at(&self, frame: FrameId, offset: usize) -> Color {
        let w = usize::from(self.canvas.width());
        crate::render::composite_layers(
            &self.palette,
            self.canvas.layers(),
            frame,
            ((offset % w) as i32, (offset / w) as i32),
        )
    }

    pub(crate) fn restore_color(&mut self, id: ColorId, color: Color) -> Result<(), DrawError> {
        self.palette.set_color(id, color)
    }

    pub(crate) fn palette_mut(&mut self) -> &mut Palette {
        &mut self.palette
    }

    /// 改名不属于作品编辑，不产生撤销记录。返回名称是否实际变化。
    /// 先校验再赋值，无效名称不会覆盖原名称；名称仍属于需要保存的项目内容。
    pub(crate) fn rename(&mut self, name: impl Into<String>) -> Result<bool, ProjectError> {
        let name = Self::validate_name(&name.into())?;
        if self.name == name {
            return Ok(false);
        }
        self.name = name;
        Ok(true)
    }

    /// 从已校验的画布与调色板恢复作品身份；文件封装由 io 管理。
    pub(crate) fn restore(
        id: Uuid,
        name: &str,
        palette: Palette,
        canvas: Canvas,
    ) -> Result<Self, ProjectError> {
        if id.is_nil() {
            return Err(ProjectError::InvalidData("项目 UUID 不能全为零".into()));
        }
        Ok(Self {
            id,
            name: Self::validate_name(name)?,
            palette,
            canvas,
        })
    }
}

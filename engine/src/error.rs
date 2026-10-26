//! 数据、文件和命令错误及转换。

use crate::{Canvas, ColorId, LayerId};
use std::fmt;

/// 核心接口的输入错误。错误由 Result 返回，调用方自己决定如何展示。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrawError {
    EmptyPalette,
    InvalidLayerId(LayerId),
    LayerLocked(LayerId),
    InvalidDimensions { width: u16, height: u16 },
    InvalidColorId(ColorId),
    OutOfBounds { x: i32, y: i32 },
}

impl fmt::Display for DrawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LayerLocked(id) => write!(f, "图层 {id} 已锁定，请先解锁"),
            Self::InvalidLayerId(id) => write!(f, "不存在图层 ID {id}"),
            Self::EmptyPalette => write!(f, "调色板至少需要一种颜色"),
            Self::InvalidDimensions { width, height } => write!(
                f,
                "画布尺寸 {width}×{height} 无效，宽高必须在 1..={} 内",
                Canvas::MAX_SIDE
            ),
            Self::InvalidColorId(index) => write!(f, "调色板中不存在颜色 ID {index}"),
            Self::OutOfBounds { x, y } => write!(f, "坐标 ({x}, {y}) 位于画布外"),
        }
    }
}

impl std::error::Error for DrawError {}

#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Png(png::EncodingError),
    Gif(gif::EncodingError),
    Drawing(DrawError),
    InvalidData(String),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "文件操作失败：{error}"),
            Self::Json(error) => write!(f, "JSON 格式错误：{error}"),
            Self::Png(error) => write!(f, "PNG 编码失败：{error}"),
            Self::Gif(error) => write!(f, "GIF 编码失败：{error}"),
            Self::Drawing(error) => write!(f, "作品数据无效：{error}"),
            Self::InvalidData(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ProjectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Png(error) => Some(error),
            Self::Gif(error) => Some(error),
            Self::Drawing(error) => Some(error),
            Self::InvalidData(_) => None,
        }
    }
}

impl From<std::io::Error> for ProjectError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<serde_json::Error> for ProjectError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
impl From<png::EncodingError> for ProjectError {
    fn from(error: png::EncodingError) -> Self {
        Self::Png(error)
    }
}
impl From<DrawError> for ProjectError {
    fn from(error: DrawError) -> Self {
        Self::Drawing(error)
    }
}

#[derive(Debug)]
pub enum EditError {
    Project(ProjectError),
    InvalidCommandState(&'static str),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Project(error) => error.fmt(f),
            Self::InvalidCommandState(operation) => write!(f, "命令当前状态不允许{operation}"),
        }
    }
}
impl std::error::Error for EditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Project(error) => Some(error),
            _ => None,
        }
    }
}
impl From<ProjectError> for EditError {
    fn from(error: ProjectError) -> Self {
        Self::Project(error)
    }
}
impl From<DrawError> for EditError {
    fn from(error: DrawError) -> Self {
        Self::Project(error.into())
    }
}

impl From<gif::EncodingError> for ProjectError {
    fn from(error: gif::EncodingError) -> Self {
        Self::Gif(error)
    }
}

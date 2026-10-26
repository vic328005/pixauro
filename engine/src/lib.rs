//! pixauro_engine 的库入口：独立的像素创作引擎 crate，不依赖桌面 UI。
//! Project 直接包含 UUID、名称、调色板和画布，并作为 JSON 保存/打开的单位。
//! 一份作品由调色板和索引画布共同组成。Engine::load/save 负责作品与历史的完整文件读写，
//! Engine::status 汇总修改及撤销状态；JSON 转换与历史结构仅在内部使用。
//! 文件路径和工具状态由桌面层管理。
//!
//! 像素只保存调色板颜色 ID，未绘制像素用 None 表示；绘制不能绕过调色板使用任意颜色。
//! 调色板颜色为 RGBA，可完全透明或半透明。修改某个条目会改变所有引用它的像素的
//! 颜色与透明度，但不会改动像素颜色引用。None 与引用 alpha=0 的颜色是不同的数据状态。
//! 本库不包含窗口、鼠标、选色状态或显示倍率，桌面编辑器通过这里的接口操作作品。
//!
//! 以下为调用示意，作为普通文本展示，不生成文档测试。
//! ```text
//! use pixauro_engine::{Color, Engine, ProjectOptions};
//! use pixauro_engine::commands::SetPixelsCommand;
//!
//! let mut engine = Engine::new(ProjectOptions {
//!     name: "我的项目".into(),
//!     palette: vec![Color::rgb(0, 0, 0), Color::rgba(255, 0, 0, 128)],
//!     ..ProjectOptions::default()
//! })?;
//! let red = engine.project().palette().entries().nth(1).unwrap().0;
//! let layer = engine.project().canvas().layers()[0].id();
//! engine.execute(SetPixelsCommand::new(layer, engine.project().canvas().frames()[0].id, vec![((2, 2), Some(red)), ((3, 2), Some(red))]))?;
//! engine.undo()?;
//! engine.redo()?;
//! let color = engine.project().color_at((2, 2))?;
//! ```

pub mod assets;
mod background_removal;
pub mod commands;
mod engine;
mod error;
mod io;
mod model;
mod render;

pub use background_removal::{
    BackgroundRemovalOptions, BackgroundRemovalOutput, remove_background,
};
pub use commands::{Command, SpriteAnimation, SpriteFrame};
pub use engine::{Engine, EngineStatus, HistoryView};
pub use error::{DrawError, EditError, ProjectError};
pub use model::{
    Animation, AnimationFrame, AnimationId, Canvas, Cel, Color, ColorId, Frame, FrameId, ImageId,
    Layer, LayerId, LayerProperties, Palette, PaletteSource, Project, ProjectOptions,
};

// 编辑器可使用该类型识别项目，无需依赖 UUID 的生成细节。
pub use uuid::Uuid as ProjectId;

pub use io::{
    GifExportOptions, PngImportOptions, PngImportProgress, PngImportStage, PngInfo, PngRaster,
    PreparedPng, SpriteFramePreview, SpriteImportProgress, SpritePreview, SpriteSlice, inspect_png,
    png_source_preview, prepare_png_import, prepare_sprite_preview,
};

//! 作品数据及其不变量，不持有编辑历史或文件操作。
mod animation;
mod canvas;
mod layer;
mod palette;
mod project;

pub use animation::{Animation, AnimationFrame, AnimationId, Cel, Frame, FrameId, ImageId};
pub use canvas::Canvas;
pub(crate) use canvas::CanvasFile;
pub(crate) use layer::ImagePixels;
pub use layer::{Layer, LayerId, LayerProperties};
pub(crate) use palette::PaletteFile;
pub use palette::{Color, ColorId, Palette, PaletteSource};
pub use project::{Project, ProjectOptions};

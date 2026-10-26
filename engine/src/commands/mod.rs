//! 内置编辑命令；公共构造接口与内部生命周期、结构历史分别组织。
mod add_palette_color;
mod animation;
mod canvas_change;
mod command;
mod import_image;
mod import_palette;
mod import_sprites;
pub use import_sprites::{SpriteAnimation, SpriteFrame};
mod layers;
mod merge_layer_down;
mod prune_palette;
mod remove_palette_color;
mod set_palette_color;
mod set_pixels;

pub use add_palette_color::AddPaletteColorCommand;
pub use animation::{AnimationCommand, AnimationEdit};
pub use command::Command;
pub(crate) use import_image::ImportImageCommand;
pub use layers::LayerCommand;
pub use merge_layer_down::MergeLayerDownCommand;
pub use prune_palette::PrunePaletteCommand;
pub use remove_palette_color::RemovePaletteColorCommand;
pub use set_palette_color::SetPaletteColorCommand;
pub use set_pixels::SetPixelsCommand;

use crate::{Canvas, DrawError, EditError, LayerId, ProjectError};

fn invalid(s: &str) -> EditError {
    ProjectError::InvalidData(s.into()).into()
}
fn require_unlocked(c: &Canvas, id: LayerId) -> Result<(), EditError> {
    if c.layer(id)?.properties.locked {
        Err(DrawError::LayerLocked(id).into())
    } else {
        Ok(())
    }
}

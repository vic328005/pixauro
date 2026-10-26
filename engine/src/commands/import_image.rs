//! 导入图像、外部颜色与新图层作为同一步历史；重做不再依赖源文件。
use super::{canvas_change::CanvasChange, import_palette::ImportPalette, invalid};
use crate::commands::command::{CommandState, sealed};
use crate::model::PaletteFile;
use crate::{Cel, Color, Command, EditError, FrameId, Layer, LayerId, LayerProperties};
use crate::{Palette, Project};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

type ImportPixels = Vec<((i32, i32), Color)>;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportImageCommand {
    frame: FrameId,
    above: LayerId,
    layer_name: String,
    label: String,
    change: Option<CanvasChange>,
    before: Option<PaletteFile>,
    after: Option<PaletteFile>,
    #[serde(skip)]
    pixels: ImportPixels,
    #[serde(skip)]
    state: CommandState,
}

impl ImportImageCommand {
    pub(crate) fn new(frame: FrameId, above: LayerId, name: String, pixels: ImportPixels) -> Self {
        Self {
            frame,
            above,
            label: format!("导入 PNG · {name}"),
            layer_name: name,
            pixels,
            change: None,
            before: None,
            after: None,
            state: CommandState::Fresh,
        }
    }

    fn apply(&self, project: &mut Project, forward: bool) -> Result<(), EditError> {
        let data = if forward { &self.after } else { &self.before };
        let mut palette =
            Palette::from_file(data.clone().ok_or_else(|| invalid("导入历史缺少调色板"))?)?;
        palette.preserve_ids(project.palette());
        let mut canvas = project.canvas().clone();
        self.change
            .as_ref()
            .ok_or_else(|| invalid("导入历史缺少图层"))?
            .apply(&mut canvas, &palette, forward)?;
        *project.canvas_mut() = canvas;
        *project.palette_mut() = palette;
        Ok(())
    }
}

impl sealed::Sealed for ImportImageCommand {
    fn restore(&mut self, project: &Project, applied: bool) -> Result<(), EditError> {
        for data in [&self.before, &self.after] {
            let palette =
                Palette::from_file(data.clone().ok_or_else(|| invalid("导入历史缺少调色板"))?)?;
            if palette
                .entries()
                .any(|(id, _)| !project.palette().was_allocated(id))
            {
                return Err(invalid("导入历史颜色 ID 无效"));
            }
        }
        self.change
            .as_ref()
            .ok_or_else(|| invalid("导入历史缺少图层"))?
            .validate(project)?;
        self.state = CommandState::restored(applied);
        Ok(())
    }
}

#[typetag::serde(name = "import_image")]
impl Command for ImportImageCommand {
    fn name(&self) -> &str {
        &self.label
    }

    fn execute(&mut self, project: &mut Project) -> Result<bool, EditError> {
        self.state.require(CommandState::Fresh, "执行")?;
        let mut canvas = project.canvas().clone();
        canvas.require_layer_frame(self.above, self.frame)?;
        canvas.check_capacity(canvas.layers().len() + 1)?;
        let position = canvas
            .layers
            .iter()
            .position(|l| l.id == self.above)
            .ok_or(crate::DrawError::InvalidLayerId(self.above))?
            + 1;
        let mut palette = ImportPalette::new(project);
        let mut pixels = Vec::with_capacity(self.pixels.len());
        for &(pos, color) in &self.pixels {
            canvas.offset(pos)?;
            if color.alpha == 0 {
                continue;
            }
            let id = palette.color(color)?;
            pixels.push((pos, id));
        }
        pixels.sort_unstable_by_key(|p| p.0);
        let mut layer = Layer {
            id: canvas.allocate_id()?,
            animation: canvas.layer(self.above)?.animation,
            properties: LayerProperties {
                name: self.layer_name.clone(),
                visible: true,
                locked: false,
                opacity: 255,
            },
            auto_link: false,
            cels: Default::default(),
            images: Default::default(),
        };
        if !pixels.is_empty() {
            let image = canvas.allocate_image()?;
            layer.images.insert(image, Arc::new(pixels));
            layer.cels.insert(self.frame, Cel { image, x: 0, y: 0 });
        }
        canvas.layers.insert(position, layer);
        let palette = palette.finish()?;
        canvas.validate(&palette, false)?;
        self.change = Some(CanvasChange::new(project.canvas(), &canvas));
        self.before = Some(project.palette().to_file());
        self.after = Some(palette.to_file());
        *project.canvas_mut() = canvas;
        *project.palette_mut() = palette;
        // 历史只保留索引图像，释放导入时的 RGBA 缓冲容量。
        self.pixels = Vec::new();
        self.state = CommandState::Applied;
        Ok(true)
    }

    fn undo(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Applied, "撤销")?;
        self.apply(project, false)?;
        self.state = CommandState::Undone;
        Ok(())
    }

    fn redo(&mut self, project: &mut Project) -> Result<(), EditError> {
        self.state.require(CommandState::Undone, "重做")?;
        self.apply(project, true)?;
        self.state = CommandState::Applied;
        Ok(())
    }
}

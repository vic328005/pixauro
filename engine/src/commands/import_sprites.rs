//! 批量导入只提交一次：失败不修改作品，重做不依赖源文件或导入草稿。
use super::{
    canvas_change::CanvasChange,
    command::{CommandState, sealed},
    import_palette::ImportPalette,
    invalid,
};
use crate::model::PaletteFile;
use crate::{
    Animation, AnimationFrame, AnimationId, Canvas, Cel, Color, Command, EditError, Engine, Frame,
    FrameId, Layer, LayerId, LayerProperties, Palette, Project,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug)]
pub struct SpriteFrame {
    pub width: u16,
    pub height: u16,
    pub x: i32,
    pub y: i32,
    pub rgba: Vec<u8>,
}
#[derive(Debug)]
pub struct SpriteAnimation {
    pub name: String,
    pub fps: f64,
    pub frames: Vec<SpriteFrame>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportSpritesCommand {
    label: String,
    change: Option<CanvasChange>,
    before: Option<PaletteFile>,
    after: Option<PaletteFile>,
    #[serde(skip)]
    source: Option<ImportSource>,
    #[serde(skip)]
    state: CommandState,
}
#[derive(Debug)]
struct ImportSource {
    above: LayerId,
    current_frame: Option<FrameId>,
    name: String,
    animations: Vec<SpriteAnimation>,
}
impl ImportSpritesCommand {
    fn apply(&self, project: &mut Project, forward: bool) -> Result<(), EditError> {
        let data = if forward { &self.after } else { &self.before };
        let mut palette =
            Palette::from_file(data.clone().ok_or_else(|| invalid("导入历史缺少调色板"))?)?;
        palette.preserve_ids(project.palette());
        let mut canvas = project.canvas().clone();
        self.change
            .as_ref()
            .ok_or_else(|| invalid("导入历史缺少画布"))?
            .apply(&mut canvas, &palette, forward)?;
        *project.canvas_mut() = canvas;
        *project.palette_mut() = palette;
        Ok(())
    }
}
impl sealed::Sealed for ImportSpritesCommand {
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
            .ok_or_else(|| invalid("导入历史缺少画布"))?
            .validate(project)?;
        self.state = CommandState::restored(applied);
        Ok(())
    }
}
#[typetag::serde(name = "import_sprites")]
impl Command for ImportSpritesCommand {
    fn name(&self) -> &str {
        &self.label
    }
    fn execute(&mut self, project: &mut Project) -> Result<bool, EditError> {
        self.state.require(CommandState::Fresh, "执行")?;
        let source = self
            .source
            .as_ref()
            .ok_or_else(|| invalid("导入数据缺失"))?;
        let mut canvas = project.canvas().clone();
        let count: usize = source.animations.iter().map(|a| a.frames.len()).sum();
        if source.animations.is_empty()
            || count == 0
            || count > 256
            || source.animations.iter().any(|a| a.frames.is_empty())
        {
            return Err(invalid("一次导入需要 1～256 个非空切片，每个动画至少一帧"));
        }
        if source.current_frame.is_some() && count != 1 {
            return Err(invalid("导入当前帧只允许一个切片"));
        }
        if source.current_frame.is_none()
            && (canvas.animations.len() + source.animations.len() > Canvas::MAX_ANIMATIONS
                || canvas.frames.len() + count > Canvas::MAX_FRAMES)
        {
            return Err(invalid("导入后将超过 256 个动画或 4096 帧"));
        }
        let layer_count = if source.current_frame.is_some() {
            1
        } else {
            source.animations.len()
        };
        canvas.check_capacity(canvas.layers.len() + layer_count)?;
        if let Some(frame) = source.current_frame {
            canvas.require_layer_frame(source.above, frame)?;
        }
        let position = canvas
            .layers
            .iter()
            .position(|l| l.id == source.above)
            .ok_or(crate::DrawError::InvalidLayerId(source.above))?
            + 1;
        let mut palette = ImportPalette::new(project);
        let mut total = 0usize;
        for (group_index, group) in source.animations.iter().enumerate() {
            let animation = if source.current_frame.is_some() {
                canvas.layer(source.above)?.animation
            } else {
                AnimationId(canvas.next_id()?)
            };
            let mut layer = Layer {
                id: canvas.allocate_id()?,
                animation,
                properties: LayerProperties {
                    name: source.name.clone(),
                    visible: true,
                    locked: false,
                    opacity: 255,
                },
                auto_link: false,
                cels: Default::default(),
                images: Default::default(),
            };
            let mut frames = Vec::new();
            for input in &group.frames {
                let area = usize::from(input.width) * usize::from(input.height);
                total = total.saturating_add(area);
                if input.width == 0
                    || input.height == 0
                    || input.width > Canvas::MAX_SIDE
                    || input.height > Canvas::MAX_SIDE
                    || input.rgba.len() != area * 4
                    || total > Canvas::MAX_TOTAL_PIXELS
                {
                    return Err(invalid("导入帧尺寸、像素长度或总像素数无效"));
                }
                if input.x.abs_diff(0) > 8192 || input.y.abs_diff(0) > 8192 {
                    return Err(invalid("帧偏移必须为 -8192～8192"));
                }
                let frame = if let Some(id) = source.current_frame {
                    canvas.frame(id)?;
                    id
                } else {
                    let id = FrameId(canvas.next_id()?);
                    canvas.frames.push(Frame { id });
                    frames.push(AnimationFrame { frame: id });
                    id
                };
                let mut pixels = Vec::new();
                for (i, p) in input.rgba.as_chunks::<4>().0.iter().enumerate() {
                    if p[3] == 0 {
                        continue;
                    }
                    let id = palette.color(Color::rgba(p[0], p[1], p[2], p[3]))?;
                    pixels.push((
                        (
                            (i % usize::from(input.width)) as i32,
                            (i / usize::from(input.width)) as i32,
                        ),
                        id,
                    ));
                }
                if !pixels.is_empty() {
                    pixels.sort_unstable_by_key(|p| p.0);
                    let image = canvas.allocate_image()?;
                    layer.images.insert(image, Arc::new(pixels));
                    layer.cels.insert(
                        frame,
                        Cel {
                            image,
                            x: input.x,
                            y: input.y,
                        },
                    );
                }
            }
            if source.current_frame.is_none() {
                let name = group.name.trim();
                if name.is_empty()
                    || name.chars().count() > 128
                    || name.chars().any(char::is_control)
                    || !Animation::valid_fps(group.fps)
                {
                    return Err(invalid("动画名称或帧率无效（FPS 必须为 0.2～100）"));
                }
                if canvas.animations.iter().any(|a| a.name == name) {
                    return Err(invalid(&format!("动画名称已存在：{name}")));
                }
                canvas.animations.push(Animation {
                    id: animation,
                    name: name.into(),
                    fps: group.fps,
                    frames,
                });
            }
            canvas.layers.insert(position + group_index, layer);
        }
        let palette = palette.finish()?;
        canvas.validate(&palette, false)?;
        self.change = Some(CanvasChange::new(project.canvas(), &canvas));
        self.before = Some(project.palette().to_file());
        self.after = Some(palette.to_file());
        *project.canvas_mut() = canvas;
        *project.palette_mut() = palette;
        self.source = None;
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
impl Engine {
    pub fn apply_sprite_import(
        &mut self,
        above: LayerId,
        current_frame: Option<FrameId>,
        name: String,
        animations: Vec<SpriteAnimation>,
    ) -> Result<LayerId, EditError> {
        let position = self
            .project()
            .canvas()
            .layers()
            .iter()
            .position(|l| l.id() == above)
            .ok_or(crate::DrawError::InvalidLayerId(above))?
            + 1;
        self.execute(ImportSpritesCommand {
            label: format!("导入资源 · {name}"),
            change: None,
            before: None,
            after: None,
            source: Some(ImportSource {
                above,
                current_frame,
                name,
                animations,
            }),
            state: CommandState::Fresh,
        })?;
        Ok(self.project().canvas().layers()[position].id())
    }
}

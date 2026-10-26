use crate::{Animation, AnimationFrame, AnimationId, Frame, FrameId, ImageId};
use crate::{ColorId, DrawError, Layer, LayerId, LayerProperties, Palette, ProjectError};
use serde::{Deserialize, Serialize};

/// 动画共享画布尺寸；每个图层只属于一个动画，同动画图层按从底到顶排列。
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    width: u16,
    height: u16,
    pub(crate) layers: Vec<Layer>,
    next_layer_id: u64,
    pub(crate) frames: Vec<Frame>,
    pub(crate) animations: Vec<Animation>,
    pub(crate) next_animation_id: u64,
}

impl Canvas {
    pub const MAX_SIDE: u16 = 4096;
    pub const MAX_LAYERS: usize = 256;
    /// 限制所有图层中独立图像的已绘制像素总量；链接 cel 不重复计数。
    pub const MAX_TOTAL_PIXELS: usize = 4096 * 4096;

    pub(crate) fn new(width: u16, height: u16) -> Result<Self, DrawError> {
        Self::validate_dimensions(width, height)?;
        Ok(Self {
            width,
            height,
            next_layer_id: 2,
            next_animation_id: 3,
            frames: vec![Frame { id: FrameId(1) }],
            animations: vec![Animation {
                id: AnimationId(2),
                name: "Animation 1".into(),
                fps: Animation::DEFAULT_FPS,
                frames: vec![AnimationFrame { frame: FrameId(1) }],
            }],
            layers: vec![Layer {
                id: LayerId(1),
                animation: AnimationId(2),
                properties: LayerProperties {
                    name: "图层 1".into(),
                    visible: true,
                    locked: false,
                    opacity: 255,
                },
                auto_link: false,
                cels: Default::default(),
                images: Default::default(),
            }],
        })
    }
    fn validate_dimensions(width: u16, height: u16) -> Result<(), DrawError> {
        if width == 0 || height == 0 || width > Self::MAX_SIDE || height > Self::MAX_SIDE {
            return Err(DrawError::InvalidDimensions { width, height });
        }
        Ok(())
    }
    pub fn width(&self) -> u16 {
        self.width
    }
    pub fn height(&self) -> u16 {
        self.height
    }
    pub fn pixel_count(&self) -> usize {
        usize::from(self.width) * usize::from(self.height)
    }
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }
    pub fn animation_layers(
        &self,
        animation: AnimationId,
    ) -> impl DoubleEndedIterator<Item = &Layer> {
        self.layers
            .iter()
            .filter(move |layer| layer.animation == animation)
    }
    pub(crate) fn require_layer_frame(
        &self,
        layer: LayerId,
        frame: FrameId,
    ) -> Result<(), ProjectError> {
        let (animation, _) = self.frame_location(frame)?;
        if self.layer(layer)?.animation != self.animations[animation].id {
            return Err(ProjectError::InvalidData("图层和帧必须属于同一动画".into()));
        }
        Ok(())
    }
    pub(crate) fn add_animation_layer(
        &mut self,
        animation: AnimationId,
    ) -> Result<(), ProjectError> {
        self.check_capacity(self.layers.len() + 1)?;
        let id = self.allocate_id()?;
        self.layers.push(Layer {
            id,
            animation,
            properties: LayerProperties {
                name: "图层 1".into(),
                visible: true,
                locked: false,
                opacity: 255,
            },
            auto_link: false,
            cels: Default::default(),
            images: Default::default(),
        });
        Ok(())
    }
    pub fn layer(&self, id: LayerId) -> Result<&Layer, DrawError> {
        self.layers
            .iter()
            .find(|l| l.id == id)
            .ok_or(DrawError::InvalidLayerId(id))
    }
    pub fn pixel_at(
        &self,
        id: LayerId,
        position: (i32, i32),
    ) -> Result<Option<ColorId>, DrawError> {
        self.offset(position)?;
        let layer = self.layer(id)?;
        let animation = self
            .animations
            .iter()
            .find(|a| a.id == layer.animation)
            .expect("图层所属动画存在");
        Ok(layer.pixel_at(animation.frames[0].frame, position))
    }
    pub(crate) fn layer_mut(&mut self, id: LayerId) -> Result<&mut Layer, DrawError> {
        self.layers
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or(DrawError::InvalidLayerId(id))
    }
    pub(crate) fn offset(&self, (x, y): (i32, i32)) -> Result<usize, DrawError> {
        if x < 0 || y < 0 || x >= i32::from(self.width) || y >= i32::from(self.height) {
            return Err(DrawError::OutOfBounds { x, y });
        }
        Ok(y as usize * usize::from(self.width) + x as usize)
    }
    pub(crate) fn was_allocated(&self, id: LayerId) -> bool {
        id.0 > 0 && id.0 < self.next_layer_id
    }
    pub(crate) fn allocate_id(&mut self) -> Result<LayerId, ProjectError> {
        let next = self
            .next_layer_id
            .checked_add(1)
            .ok_or_else(|| ProjectError::InvalidData("图层 ID 已耗尽".into()))?;
        let id = LayerId(self.next_layer_id);
        self.next_layer_id = next;
        Ok(id)
    }
    pub(crate) fn check_capacity(&self, len: usize) -> Result<(), ProjectError> {
        if len == 0 || len > Self::MAX_LAYERS {
            return Err(ProjectError::InvalidData(
                "须保留至少一个图层，最多 256 层".into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn to_file(&self) -> CanvasFile {
        CanvasFile {
            width: self.width,
            height: self.height,
            next_layer_id: self.next_layer_id,
            frames: self.frames.clone(),
            animations: self.animations.clone(),
            next_animation_id: self.next_animation_id,
            layers: self.layers.clone(),
        }
    }
    pub(crate) fn from_file(file: CanvasFile, palette: &Palette) -> Result<Self, ProjectError> {
        Self::validate_dimensions(file.width, file.height)?;
        let mut canvas = Self {
            width: file.width,
            height: file.height,
            next_layer_id: file.next_layer_id,
            frames: file.frames,
            animations: file.animations,
            next_animation_id: file.next_animation_id,
            layers: Vec::new(),
        };
        canvas.check_capacity(file.layers.len())?;
        for layer in file.layers {
            if !canvas.was_allocated(layer.id) || canvas.layers.iter().any(|l| l.id == layer.id) {
                return Err(ProjectError::InvalidData("图层 ID 无效或重复".into()));
            }
            layer.validate(palette, false)?;
            canvas.layers.push(layer);
        }
        canvas.validate(palette, false)?;
        Ok(canvas)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CanvasFile {
    width: u16,
    height: u16,
    next_layer_id: u64,
    layers: Vec<Layer>,
    frames: Vec<Frame>,
    animations: Vec<Animation>,
    next_animation_id: u64,
}

impl Canvas {
    pub const MAX_FRAMES: usize = 4096;
    pub const MAX_ANIMATIONS: usize = 256;
    pub fn animations(&self) -> &[Animation] {
        &self.animations
    }
    pub fn animation(&self, id: AnimationId) -> Result<usize, ProjectError> {
        self.animations
            .iter()
            .position(|a| a.id == id)
            .ok_or_else(|| ProjectError::InvalidData("动画不存在".into()))
    }
    pub(crate) fn frame_location(&self, id: FrameId) -> Result<(usize, usize), ProjectError> {
        self.animations
            .iter()
            .enumerate()
            .find_map(|(a, animation)| {
                animation
                    .frames
                    .iter()
                    .position(|f| f.frame == id)
                    .map(|f| (a, f))
            })
            .ok_or_else(|| ProjectError::InvalidData("动画中不存在此帧".into()))
    }
    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }
    pub fn frame(&self, id: FrameId) -> Result<usize, ProjectError> {
        self.frames
            .iter()
            .position(|f| f.id == id)
            .ok_or_else(|| ProjectError::InvalidData("帧不存在".into()))
    }
    pub(crate) fn next_id(&mut self) -> Result<u64, ProjectError> {
        let id = self.next_animation_id;
        self.next_animation_id = id
            .checked_add(1)
            .ok_or_else(|| ProjectError::InvalidData("动画 ID 已耗尽".into()))?;
        Ok(id)
    }
    pub(crate) fn allocate_image(&mut self) -> Result<ImageId, ProjectError> {
        Ok(ImageId(self.next_id()?))
    }
    pub(crate) fn validate(&self, palette: &Palette, historical: bool) -> Result<(), ProjectError> {
        self.check_capacity(self.layers.len())?;
        let bad = || ProjectError::InvalidData("动画帧、图像或身份无效".into());
        if self.frames.is_empty() || self.frames.len() > Self::MAX_FRAMES {
            return Err(bad());
        }
        let frames: std::collections::BTreeSet<_> = self.frames.iter().map(|f| f.id).collect();
        let valid = |id: u64| id > 0 && id < self.next_animation_id;
        if frames.len() != self.frames.len() || self.frames.iter().any(|f| !valid(f.id.0)) {
            return Err(bad());
        }
        if self.animations.is_empty() || self.animations.len() > Self::MAX_ANIMATIONS {
            return Err(bad());
        }
        let mut animation_ids = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        let mut assigned = std::collections::BTreeSet::new();
        let mut frame_owners = std::collections::BTreeMap::new();
        for a in &self.animations {
            if !valid(a.id.0)
                || !animation_ids.insert(a.id)
                || !names.insert(&a.name)
                || a.name.trim().is_empty()
                || a.name.trim() != a.name
                || a.name.chars().count() > 128
                || a.name.chars().any(char::is_control)
                || !Animation::valid_fps(a.fps)
                || a.frames.is_empty()
                || !self.layers.iter().any(|l| l.animation == a.id)
            {
                return Err(bad());
            }
            for f in &a.frames {
                frame_owners.insert(f.frame, a.id);
                if !frames.contains(&f.frame) || !assigned.insert(f.frame) {
                    return Err(bad());
                }
            }
        }
        if assigned != frames {
            return Err(bad());
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut total = 0;
        for layer in &self.layers {
            if !self.was_allocated(layer.id)
                || !ids.insert(layer.id)
                || !animation_ids.contains(&layer.animation)
            {
                return Err(bad());
            }
            layer.validate(palette, historical)?;
            if layer
                .cels
                .keys()
                .any(|id| frame_owners.get(id) != Some(&layer.animation))
                || layer.images.keys().any(|id| !valid(id.0))
            {
                return Err(bad());
            }
            let mut image_owners = std::collections::BTreeMap::new();
            for (frame, cel) in &layer.cels {
                let owner = frame_owners[frame];
                if image_owners
                    .insert(cel.image, owner)
                    .is_some_and(|previous| previous != owner)
                {
                    return Err(bad());
                }
            }
            total += layer.images.values().map(|p| p.len()).sum::<usize>();
        }
        if total > Self::MAX_TOTAL_PIXELS {
            return Err(bad());
        }
        Ok(())
    }
}

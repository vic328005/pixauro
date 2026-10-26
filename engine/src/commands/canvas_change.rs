//! 动画与图层合并共用的结构差量，保留受影响图层及帧顺序。
use super::invalid;
use crate::{Animation, AnimationId, Canvas, EditError, Frame, Layer, LayerId, Palette, Project};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Structure {
    layers: Vec<(LayerId, AnimationId)>,
    frames: Vec<Frame>,
    animations: Vec<Animation>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct CanvasChange {
    before: Structure,
    after: Structure,
    old_layers: Vec<Layer>,
    new_layers: Vec<Layer>,
}
impl Structure {
    fn new(c: &Canvas) -> Self {
        Self {
            layers: c.layers.iter().map(|l| (l.id, l.animation)).collect(),
            frames: c.frames.clone(),
            animations: c.animations.clone(),
        }
    }
}
impl CanvasChange {
    pub(crate) fn new(before: &Canvas, after: &Canvas) -> Self {
        Self {
            before: Structure::new(before),
            after: Structure::new(after),
            old_layers: before
                .layers
                .iter()
                .filter(|l| after.layer(l.id).ok() != Some(*l))
                .cloned()
                .collect(),
            new_layers: after
                .layers
                .iter()
                .filter(|l| before.layer(l.id).ok() != Some(*l))
                .cloned()
                .collect(),
        }
    }
    pub(crate) fn validate(&self, project: &Project) -> Result<(), EditError> {
        for (s, layers) in [
            (&self.before, &self.old_layers),
            (&self.after, &self.new_layers),
        ] {
            let c = project.canvas();
            if s.layers.is_empty()
                || s.layers.len() > Canvas::MAX_LAYERS
                || s.layers.iter().any(|(id, _)| !c.was_allocated(*id))
                || s.layers
                    .iter()
                    .map(|(id, _)| id)
                    .collect::<BTreeSet<_>>()
                    .len()
                    != s.layers.len()
            {
                return Err(invalid("动画历史图层无效"));
            }
            let mut check = c.clone();
            check.layers = s
                .layers
                .iter()
                .map(|(id, animation)| Layer {
                    id: *id,
                    animation: *animation,
                    properties: crate::LayerProperties {
                        name: "历史校验".into(),
                        visible: true,
                        locked: false,
                        opacity: 255,
                    },
                    auto_link: false,
                    cels: Default::default(),
                    images: Default::default(),
                })
                .collect();
            check.frames = s.frames.clone();
            check.animations = s.animations.clone();
            for layer in layers {
                let slot = check
                    .layers
                    .iter_mut()
                    .find(|l| l.id == layer.id)
                    .ok_or_else(|| invalid("动画历史包含未知图层"))?;
                if slot.animation != layer.animation {
                    return Err(invalid("动画历史图层归属不一致"));
                }
                *slot = layer.clone();
            }
            check.validate(project.palette(), true)?;
        }
        Ok(())
    }
    pub(crate) fn apply(
        &self,
        c: &mut Canvas,
        palette: &Palette,
        forward: bool,
    ) -> Result<(), EditError> {
        let (s, replacements) = if forward {
            (&self.after, &self.new_layers)
        } else {
            (&self.before, &self.old_layers)
        };
        let mut next = c.clone();
        next.layers = s
            .layers
            .iter()
            .map(|(id, animation)| {
                let layer = replacements
                    .iter()
                    .find(|l| l.id == *id)
                    .or_else(|| c.layers.iter().find(|l| l.id == *id))
                    .cloned()
                    .ok_or_else(|| invalid("动画历史图层缺失"))?;
                if layer.animation != *animation {
                    return Err(invalid("动画历史图层归属不一致"));
                }
                Ok(layer)
            })
            .collect::<Result<_, _>>()?;
        next.frames = s.frames.clone();
        next.animations = s.animations.clone();
        next.validate(palette, false)?;
        *c = next;
        Ok(())
    }
}

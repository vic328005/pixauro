//! 动画编辑意图与画布变更；生命周期及历史由 AnimationCommand 管理。
use super::super::{invalid, require_unlocked};
use crate::{Animation, AnimationFrame, AnimationId, Canvas, EditError, Frame, FrameId, LayerId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AnimationEdit {
    AddAnimation {
        name: String,
    },
    RenameAnimation {
        id: AnimationId,
        name: String,
    },
    DeleteAnimation {
        id: AnimationId,
    },
    MoveAnimation {
        id: AnimationId,
        position: usize,
    },
    NewFrame {
        after: FrameId,
    },
    DuplicateFrame {
        frame: FrameId,
        linked: bool,
    },
    DeleteFrame {
        frame: FrameId,
    },
    Timing {
        animation: AnimationId,
        fps: f64,
    },
    AutoLink {
        layer: LayerId,
        on: bool,
    },
    Link {
        layer: LayerId,
        frames: Vec<FrameId>,
    },
    LinkPrevious {
        layer: LayerId,
        frame: FrameId,
    },
    Unlink {
        layer: LayerId,
        frames: Vec<FrameId>,
    },
    Clear {
        layer: LayerId,
        frames: Vec<FrameId>,
    },
    Offset {
        layer: LayerId,
        frame: FrameId,
        x: i32,
        y: i32,
    },
}

impl AnimationEdit {
    pub(super) fn apply(&self, c: &mut Canvas) -> Result<String, EditError> {
        use AnimationEdit::*;
        match self {
            AddAnimation { name } => {
                if c.animations.len() >= Canvas::MAX_ANIMATIONS
                    || c.frames.len() >= Canvas::MAX_FRAMES
                {
                    return Err(invalid("最多 256 个动画、4096 帧"));
                }
                let name = animation_name(c, None, name)?;
                let id = AnimationId(c.next_id()?);
                let frame = FrameId(c.next_id()?);
                c.frames.push(Frame { id: frame });
                c.animations.push(Animation {
                    id,
                    name: name.clone(),
                    fps: Animation::DEFAULT_FPS,
                    frames: vec![AnimationFrame { frame }],
                });
                c.add_animation_layer(id)?;
                Ok(format!("新建动画 · {name}"))
            }
            RenameAnimation { id, name } => {
                let i = c.animation(*id)?;
                let name = animation_name(c, Some(*id), name)?;
                c.animations[i].name = name.clone();
                Ok(format!("重命名动画 · {name}"))
            }
            MoveAnimation { id, position } => {
                let from = c.animation(*id)?;
                if *position >= c.animations.len() {
                    return Err(invalid("动画位置超出范围"));
                }
                let animation = c.animations.remove(from);
                let label = format!("动画排序 · {}", animation.name);
                c.animations.insert(*position, animation);
                Ok(label)
            }
            DeleteAnimation { id } => {
                if c.animations.len() == 1 {
                    return Err(invalid("至少保留一个动画"));
                }
                let i = c.animation(*id)?;
                let frames: std::collections::BTreeSet<_> =
                    c.animations[i].frames.iter().map(|f| f.frame).collect();
                for l in c.animation_layers(*id) {
                    require_unlocked(c, l.id)?;
                }
                let animation = c.animations.remove(i);
                c.frames.retain(|f| !frames.contains(&f.id));
                c.layers.retain(|l| l.animation != *id);
                Ok(format!("删除动画 · {}", animation.name))
            }
            NewFrame { after } | DuplicateFrame { frame: after, .. } => {
                if c.frames.len() >= Canvas::MAX_FRAMES {
                    return Err(invalid("最多 4096 帧"));
                }
                let i = c.frame(*after)?;
                let id = FrameId(c.next_id()?);
                let (animation, position) = c.frame_location(*after)?;
                let duplicate = matches!(*self, DuplicateFrame { .. });
                for k in 0..c.layers.len() {
                    if c.layers[k].animation != c.animations[animation].id
                        || (!duplicate && !c.layers[k].auto_link)
                    {
                        continue;
                    }
                    if let Some(mut cel) = c.layers[k].cels.get(after).cloned() {
                        if matches!(*self, DuplicateFrame { linked: false, .. }) {
                            let pixels = c.layers[k].images[&cel.image].clone();
                            cel.image = c.allocate_image()?;
                            c.layers[k].images.insert(cel.image, pixels);
                        }
                        c.layers[k].cels.insert(id, cel);
                    }
                }
                c.frames.insert(i + 1, Frame { id });
                c.animations[animation]
                    .frames
                    .insert(position + 1, AnimationFrame { frame: id });
                Ok(format!(
                    "{} · 帧 {}",
                    if duplicate {
                        if matches!(*self, DuplicateFrame { linked: true, .. }) {
                            "链接复制帧"
                        } else {
                            "独立复制帧"
                        }
                    } else {
                        "新建帧"
                    },
                    position + 2
                ))
            }
            DeleteFrame { frame } => {
                let (animation, position) = c.frame_location(*frame)?;
                if c.animations[animation].frames.len() == 1 {
                    return Err(invalid("动画至少保留一帧"));
                }
                let i = c.frame(*frame)?;
                for l in &c.layers {
                    if l.cels.contains_key(frame) {
                        require_unlocked(c, l.id)?;
                    }
                }
                c.frames.remove(i);
                c.animations[animation].frames.remove(position);
                let owner = c.animations[animation].id;
                for l in c.layers.iter_mut().filter(|l| l.animation == owner) {
                    l.cels.remove(frame);
                    l.collect_images();
                }
                Ok(format!("删除帧 · {}", position + 1))
            }
            Timing { animation, fps } => {
                if !Animation::valid_fps(*fps) {
                    return Err(invalid("动画帧率必须为 0.2～100 FPS"));
                }
                let a = c.animation(*animation)?;
                c.animations[a].fps = *fps;
                Ok(format!("动画帧率 · {} · {fps} FPS", c.animations[a].name))
            }
            AutoLink { layer, on } => {
                let l = c.layer_mut(*layer)?;
                l.auto_link = *on;
                Ok(format!(
                    "新帧{} · {}",
                    if *on { "链接" } else { "留空" },
                    l.properties.name
                ))
            }
            LinkPrevious { layer, frame } => {
                require_unlocked(c, *layer)?;
                c.require_layer_frame(*layer, *frame)?;
                let (a, i) = c.frame_location(*frame)?;
                let prev = i
                    .checked_sub(1)
                    .ok_or_else(|| invalid("此动画没有前一帧"))?;
                let cel = c
                    .layer(*layer)?
                    .cels
                    .get(&c.animations[a].frames[prev].frame)
                    .cloned()
                    .ok_or_else(|| invalid("前一帧为空"))?;
                let l = c.layer_mut(*layer)?;
                if l.cels.get(frame).is_none_or(|v| v.image != cel.image) {
                    l.cels.insert(*frame, cel);
                    l.collect_images();
                }
                Ok(format!(
                    "链接到前一帧 · {} · 帧 {}",
                    l.properties.name,
                    i + 1
                ))
            }
            Link { layer, frames } | Unlink { layer, frames } | Clear { layer, frames } => {
                require_unlocked(c, *layer)?;
                let owner = frames
                    .first()
                    .map(|f| c.frame_location(*f))
                    .transpose()?
                    .map(|(a, _)| a);
                let mut list = frames
                    .iter()
                    .map(|f| {
                        c.require_layer_frame(*layer, *f)?;
                        let (a, i) = c.frame_location(*f)?;
                        if Some(a) != owner {
                            return Err(invalid("请选择同一动画内的帧"));
                        }
                        Ok((i, *f))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                list.sort();
                list.dedup();
                let anchor = list
                    .iter()
                    .find_map(|(_, f)| c.layer(*layer).ok()?.cels.get(f).cloned());
                for (_, f) in &list {
                    if matches!(*self, Link { .. }) {
                        if let Some(cel) = &anchor {
                            c.layer_mut(*layer)?.cels.insert(*f, cel.clone());
                        }
                    } else if matches!(*self, Clear { .. }) {
                        c.layer_mut(*layer)?.cels.remove(f);
                    } else if let Some(mut cel) = c.layer(*layer)?.cels.get(f).cloned() {
                        let l = c.layer(*layer)?;
                        if l.cels.values().filter(|v| v.image == cel.image).count() > 1 {
                            let pixels = l.images[&cel.image].clone();
                            cel.image = c.allocate_image()?;
                            let l = c.layer_mut(*layer)?;
                            l.images.insert(cel.image, pixels);
                            l.cels.insert(*f, cel);
                        }
                    }
                }
                let l = c.layer_mut(*layer)?;
                l.collect_images();
                Ok(format!(
                    "{} cel · {} · 帧 {}",
                    match *self {
                        Link { .. } => "链接",
                        Unlink { .. } => "取消链接",
                        _ => "清除",
                    },
                    l.properties.name,
                    list.iter()
                        .map(|(i, _)| (i + 1).to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }
            Offset { layer, frame, x, y } => {
                require_unlocked(c, *layer)?;
                c.require_layer_frame(*layer, *frame)?;
                let (_, i) = c.frame_location(*frame)?;
                if x.abs_diff(0) > 8192 || y.abs_diff(0) > 8192 {
                    return Err(invalid("偏移必须为 -8192～8192"));
                }
                let l = c.layer_mut(*layer)?;
                let cel = l
                    .cels
                    .get_mut(frame)
                    .ok_or_else(|| invalid("空 cel 无法移动"))?;
                cel.x = *x;
                cel.y = *y;
                Ok(format!("移动 cel · {} · 帧 {}", l.properties.name, i + 1))
            }
        }
    }
}

fn animation_name(
    c: &Canvas,
    except: Option<AnimationId>,
    value: &str,
) -> Result<String, EditError> {
    let name = value.trim();
    if name.is_empty() || name.chars().count() > 128 || name.chars().any(char::is_control) {
        return Err(invalid("动画名称须为 1～128 个字符，且不能包含控制字符"));
    }
    if c.animations
        .iter()
        .any(|a| Some(a.id) != except && a.name == name)
    {
        return Err(invalid("动画名称不能重复"));
    }
    Ok(name.into())
}

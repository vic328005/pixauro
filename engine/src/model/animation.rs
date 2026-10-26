//! 动画身份与数据；播放位置和选择属于编辑器，不写入作品。
use serde::{Deserialize, Serialize};
macro_rules! identity {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub(crate) u64);
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
identity!(FrameId);
identity!(ImageId);
identity!(AnimationId);
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub id: FrameId,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cel {
    pub image: ImageId,
    pub x: i32,
    pub y: i32,
}

/// 帧只有内容身份；序列顺序与时间属于动画。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationFrame {
    pub frame: FrameId,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Animation {
    pub id: AnimationId,
    pub name: String,
    pub fps: f64,
    pub frames: Vec<AnimationFrame>,
}

impl Animation {
    pub const DEFAULT_FPS: f64 = 10.0;
    pub const MIN_FPS: f64 = 0.2;
    pub const MAX_FPS: f64 = 100.0;

    pub(crate) fn valid_fps(fps: f64) -> bool {
        fps.is_finite() && (Self::MIN_FPS..=Self::MAX_FPS).contains(&fps)
    }
}

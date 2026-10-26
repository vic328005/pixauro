//! 图层持有按帧寻址的 cel 与层内共享图像；像素始终引用全局调色板。
use crate::model::{AnimationId, Cel, FrameId, ImageId};
use crate::{ColorId, Palette, ProjectError};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub(crate) u64);
impl std::fmt::Display for LayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerProperties {
    /// 显示名称，允许重名，不用作图层查找键。
    pub name: String,
    /// 是否参与作品合成；隐藏不会删除像素，也不会阻止 engine 接受该层的编辑。
    /// 单层 PNG 导出忽略此开关，作品 PNG 导出遵循此开关。
    pub visible: bool,
    /// 编辑保护：禁止新的像素写入和删除该层，仍允许属性修改、排序及复制。
    /// 撤销/重做按历史顺序恢复数据；共享调色板改色及颜色替换仍影响全部引用层。
    pub locked: bool,
    /// 图层不透明度：0 为完全透明，255 为完全不透明。
    /// 合成时有效 Alpha 为颜色 alpha × opacity / 65025，不改写原始颜色引用。
    /// 单层 PNG 会应用此值；单层数据导出保留属性和未应用透明度的原始索引。
    pub opacity: u8,
}

impl LayerProperties {
    /// 检查名称有效性，但不修改或修剪名称。
    /// 编辑命令在调用前处理首尾空白；文件加载只校验，不重新规范化名称。
    /// 长度按 Rust char（Unicode 标量值）计数，不按 UTF-8 字节数计算。
    /// visible、locked 和 opacity 的类型已限定取值范围，不需要额外范围检查。
    pub(crate) fn validate(&self) -> Result<(), ProjectError> {
        // 拒绝空串、全空白、过长名称以及换行等控制字符。
        if self.name.trim().is_empty()
            || self.name.chars().count() > 128
            || self.name.chars().any(char::is_control)
        {
            return Err(ProjectError::InvalidData(
                "图层名称须为 1～128 个字符，且不能包含控制字符".into(),
            ));
        }
        Ok(())
    }
}

/// 稀疏图像按 (x,y) 排序，允许负坐标，移动不裁掉画布外的内容。
pub type ImagePixels = Vec<((i32, i32), ColorId)>;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub(crate) id: LayerId,
    pub(crate) animation: AnimationId,
    pub(crate) properties: LayerProperties,
    pub(crate) auto_link: bool,
    pub(crate) cels: BTreeMap<FrameId, Cel>,
    pub(crate) images: BTreeMap<ImageId, Arc<ImagePixels>>,
}
impl Layer {
    pub fn id(&self) -> LayerId {
        self.id
    }
    pub fn animation(&self) -> AnimationId {
        self.animation
    }
    pub fn properties(&self) -> &LayerProperties {
        &self.properties
    }
    pub fn auto_link(&self) -> bool {
        self.auto_link
    }
    pub fn cels(&self) -> &BTreeMap<FrameId, Cel> {
        &self.cels
    }
    pub fn images(&self) -> &BTreeMap<ImageId, Arc<ImagePixels>> {
        &self.images
    }
    pub fn pixel_at(&self, frame: FrameId, (x, y): (i32, i32)) -> Option<ColorId> {
        let cel = self.cels.get(&frame)?;
        let pixels = self.images.get(&cel.image)?;
        let key = (x - cel.x, y - cel.y);
        pixels
            .binary_search_by_key(&key, |p| p.0)
            .ok()
            .map(|i| pixels[i].1)
    }
    pub(crate) fn collect_images(&mut self) {
        let used: std::collections::BTreeSet<_> = self.cels.values().map(|c| c.image).collect();
        self.images.retain(|id, _| used.contains(id));
    }
    pub(crate) fn validate(&self, palette: &Palette, historical: bool) -> Result<(), ProjectError> {
        self.properties.validate()?;
        if self.cels.values().any(|c| {
            !self.images.contains_key(&c.image) || c.x.abs_diff(0) > 8192 || c.y.abs_diff(0) > 8192
        }) {
            return Err(ProjectError::InvalidData("cel 图像引用或偏移无效".into()));
        }
        let total: usize = self.images.values().map(|p| p.len()).sum();
        if total > crate::Canvas::MAX_TOTAL_PIXELS {
            return Err(ProjectError::InvalidData("图像像素数量超限".into()));
        }
        for pixels in self.images.values() {
            if pixels.windows(2).any(|p| p[0].0 >= p[1].0)
                || pixels.iter().any(|((x, y), id)| {
                    x.abs_diff(0) > 12288
                        || y.abs_diff(0) > 12288
                        || if historical {
                            !palette.was_allocated(*id)
                        } else {
                            palette.color(*id).is_none()
                        }
                })
            {
                return Err(ProjectError::InvalidData(
                    "图像坐标、顺序或颜色引用无效".into(),
                ));
            }
        }
        Ok(())
    }
}

use serde::{Deserialize, Serialize};

use crate::{DrawError, ProjectError};

/// RGBA 颜色，每个通道使用 u8，范围为 0..=255。
/// alpha=0 为完全透明，255 为完全不透明，128 约为半透明。
/// RGB 保存原始颜色，不预先乘 alpha，修改透明度时不损失原来的 RGB 值。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    /// 创建不透明颜色，保留原来的 RGB 调用方式，等同于 rgba(red, green, blue, 255)。
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self::rgba(red, green, blue, 255)
    }

    /// 创建带透明度的颜色。它仍是调色板中的一个条目，可以被多个像素引用。
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }
}

/// 颜色在项目内的稳定身份，与显示位置无关。删除后不复用，撤销时恢复原 ID。
/// 调用方从 Palette::entries 获取 ID，不能把色块位置直接当成 ID 使用。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ColorId(u64);

impl std::fmt::Display for ColorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// 颜色来源只负责分区，不改变像素的 ColorId 引用规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaletteSource {
    Custom,
    External,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PaletteColor {
    color: Color,
    source: PaletteSource,
}

/// 有序映射同时保存稳定身份和用户看到的色块顺序。
/// 像素只引用 ColorId，删除条目不会改变其他颜色的含义。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    colors: indexmap::IndexMap<ColorId, PaletteColor>,
    next_id: u64,
}

impl Palette {
    pub fn new(colors: Vec<Color>) -> Result<Self, DrawError> {
        if colors.is_empty() {
            return Err(DrawError::EmptyPalette);
        }
        Ok(Self {
            next_id: colors.len() as u64 + 1,
            colors: colors
                .into_iter()
                .enumerate()
                .map(|(i, color)| {
                    (
                        ColorId(i as u64 + 1),
                        PaletteColor {
                            color,
                            source: PaletteSource::Custom,
                        },
                    )
                })
                .collect(),
        })
    }

    /// 按调色板显示顺序返回稳定 ID 和 RGBA；不暴露内部容器或可变引用。
    pub fn entries(
        &self,
    ) -> impl ExactSizeIterator<Item = (ColorId, Color)> + DoubleEndedIterator + '_ {
        self.colors.iter().map(|(&id, entry)| (id, entry.color))
    }

    pub fn color(&self, id: ColorId) -> Option<Color> {
        self.colors.get(&id).map(|entry| entry.color)
    }

    pub fn source(&self, id: ColorId) -> Option<PaletteSource> {
        self.colors.get(&id).map(|entry| entry.source)
    }

    pub(crate) fn set_color(&mut self, id: ColorId, color: Color) -> Result<(), DrawError> {
        self.colors
            .get_mut(&id)
            .ok_or(DrawError::InvalidColorId(id))?
            .color = color;
        Ok(())
    }

    // 历史中的颜色可能已经被删除。加载只检查 ID 是否属于分配范围，
    // 不要求它此刻仍在调色板中；真正执行 undo/redo 时再检查当前引用。
    pub(crate) fn was_allocated(&self, id: ColorId) -> bool {
        id.0 > 0 && id.0 < self.next_id
    }

    pub(crate) fn append(
        &mut self,
        color: Color,
        source: PaletteSource,
    ) -> Result<ColorId, ProjectError> {
        let next = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| ProjectError::InvalidData("颜色 ID 已耗尽".into()))?;
        let id = ColorId(self.next_id);
        self.colors.insert(id, PaletteColor { color, source });
        self.next_id = next;
        Ok(id)
    }

    pub(crate) fn insert(
        &mut self,
        position: usize,
        id: ColorId,
        color: Color,
        source: PaletteSource,
    ) -> Result<(), ProjectError> {
        if position > self.colors.len() || self.colors.contains_key(&id) || !self.was_allocated(id)
        {
            return Err(ProjectError::InvalidData(
                "无法恢复颜色：位置或 ID 无效".into(),
            ));
        }
        self.colors
            .shift_insert(position, id, PaletteColor { color, source });
        Ok(())
    }

    pub(crate) fn remove(&mut self, id: ColorId) -> Result<(), DrawError> {
        if self.colors.len() <= 1 {
            return Err(DrawError::EmptyPalette);
        }
        self.colors
            .shift_remove(&id)
            .ok_or(DrawError::InvalidColorId(id))?;
        Ok(())
    }

    pub(crate) fn preserve_ids(&mut self, other: &Self) {
        self.next_id = self.next_id.max(other.next_id);
    }

    pub(crate) fn to_file(&self) -> PaletteFile {
        PaletteFile {
            next_id: self.next_id,
            colors: self
                .entries()
                .map(|(id, c)| PaletteEntry {
                    id,
                    source: self.source(id).expect("颜色存在"),
                    color: [c.red, c.green, c.blue, c.alpha],
                })
                .collect(),
        }
    }

    pub(crate) fn from_file(data: PaletteFile) -> Result<Self, ProjectError> {
        if data.colors.is_empty() {
            return Err(DrawError::EmptyPalette.into());
        }
        let mut palette = Self {
            colors: indexmap::IndexMap::new(),
            next_id: data.next_id,
        };
        for entry in data.colors {
            let [r, g, b, a] = entry.color;
            palette.insert(
                palette.colors.len(),
                entry.id,
                Color::rgba(r, g, b, a),
                entry.source,
            )?;
        }
        Ok(palette)
    }
}

/// 使用显式条目数组保存顺序，不依赖 JSON 对象的键顺序。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PaletteFile {
    next_id: u64,
    colors: Vec<PaletteEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PaletteEntry {
    id: ColorId,
    source: PaletteSource,
    color: [u8; 4],
}

impl Default for Palette {
    fn default() -> Self {
        // 默认作品的 8 色方案，覆盖深浅底色及常用色相。UI 从作品读取它，不再持有独立的全局调色板。
        let rgb = [
            0x172038, 0xf4f4e8, 0xe85d75, 0xffd166, 0x62bd69, 0x50c5b7, 0x3769bd, 0x8257aa,
        ];
        Self::new(
            rgb.into_iter()
                .map(|value| Color::rgb((value >> 16) as u8, (value >> 8) as u8, value as u8))
                .collect(),
        )
        .expect("默认调色板非空")
    }
}

//! 唯一的 Rust → TypeScript 边界。稳定颜色 ID 和版本均显式使用字符串，
//! 不能让 JavaScript 的浮点整数精度截断 Engine 的 u64 身份。

use pixauro_engine::{Color, Engine, ProjectOptions};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Deserialize, Serialize, Type)]
pub struct RgbaColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl From<Color> for RgbaColor {
    fn from(color: Color) -> Self {
        Self {
            red: color.red,
            green: color.green,
            blue: color.blue,
            alpha: color.alpha,
        }
    }
}
impl From<RgbaColor> for Color {
    fn from(color: RgbaColor) -> Self {
        Self::rgba(color.red, color.green, color.blue, color.alpha)
    }
}

#[derive(Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentGuard {
    pub session_id: String,
    pub revision: String,
}

#[derive(Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewProjectOptions {
    pub name: String,
    pub width: u16,
    pub height: u16,
}

impl From<NewProjectOptions> for ProjectOptions {
    fn from(value: NewProjectOptions) -> Self {
        Self {
            name: value.name,
            width: value.width,
            height: value.height,
            ..Self::default()
        }
    }
}

#[derive(Clone, Deserialize, Serialize, Type)]
pub struct RecentProject {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPreview {
    pub name: String,
    pub width: u16,
    pub height: u16,
    pub updated_at: Option<f64>,
    pub preview_width: u16,
    pub preview_height: u16,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PaletteSource {
    Custom,
    External,
}

impl From<pixauro_engine::PaletteSource> for PaletteSource {
    fn from(source: pixauro_engine::PaletteSource) -> Self {
        match source {
            pixauro_engine::PaletteSource::Custom => Self::Custom,
            pixauro_engine::PaletteSource::External => Self::External,
        }
    }
}

#[derive(Clone, Serialize, Type)]
pub struct PaletteEntry {
    pub id: String,
    pub color: RgbaColor,
    pub usage: u32,
    pub source: PaletteSource,
}

#[derive(Clone, Serialize, Type)]
pub struct HistorySnapshot {
    pub entries: Vec<String>,
    pub position: u32,
}

#[derive(Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSnapshot {
    pub session_id: String,
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub width: u16,
    pub height: u16,
    pub layers: Vec<LayerSnapshot>,
    pub frames: Vec<FrameSnapshot>,
    pub animations: Vec<AnimationSnapshot>,
    pub palette: Vec<PaletteEntry>,
    pub history: HistorySnapshot,
    pub modified: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub revision: String,
}

#[derive(Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LayerSnapshot {
    pub id: String,
    pub animation_id: String,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub opacity: u8,
    pub auto_link: bool,
    pub cels: Vec<CelSnapshot>,
    pub images: Vec<ImageSnapshot>,
}

#[derive(Clone, Deserialize, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum LayerEdit {
    Add {
        name: String,
        animation: String,
        above: Option<String>,
    },
    Duplicate {
        id: String,
        name: String,
    },
    Remove {
        id: String,
    },
    MergeDown {
        id: String,
    },
    Properties {
        id: String,
        name: String,
        visible: bool,
        locked: bool,
        opacity: u8,
    },
    Move {
        id: String,
        position: u32,
    },
}

impl DocumentSnapshot {
    pub(crate) fn new(engine: &Engine, session_id: &str, path: Option<&std::path::Path>) -> Self {
        let project = engine.project();
        let status = engine.status();
        let mut counts = std::collections::HashMap::new();
        for id in project.canvas().layers().iter().flat_map(|layer| {
            layer
                .images()
                .values()
                .flat_map(|pixels| pixels.iter().map(|(_, id)| id))
        }) {
            *counts.entry(*id).or_insert(0_u32) += 1;
        }
        Self {
            session_id: session_id.to_owned(),
            id: project.id().to_string(),
            name: project.name().to_owned(),
            path: path.map(|p| p.to_string_lossy().into_owned()),
            width: project.canvas().width(),
            height: project.canvas().height(),
            layers: project
                .canvas()
                .layers()
                .iter()
                .map(|layer| LayerSnapshot {
                    id: layer.id().to_string(),
                    animation_id: layer.animation().to_string(),
                    name: layer.properties().name.clone(),
                    visible: layer.properties().visible,
                    locked: layer.properties().locked,
                    opacity: layer.properties().opacity,
                    auto_link: layer.auto_link(),
                    cels: layer
                        .cels()
                        .iter()
                        .map(|(frame, cel)| CelSnapshot {
                            frame_id: frame.to_string(),
                            image_id: cel.image.to_string(),
                            x: cel.x,
                            y: cel.y,
                        })
                        .collect(),
                    images: layer
                        .images()
                        .iter()
                        .map(|(id, pixels)| ImageSnapshot {
                            id: id.to_string(),
                            pixels: pixels
                                .iter()
                                .map(|((x, y), id)| (*x, *y, id.to_string()))
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
            frames: project
                .canvas()
                .frames()
                .iter()
                .map(|f| FrameSnapshot {
                    id: f.id.to_string(),
                })
                .collect(),
            animations: project
                .canvas()
                .animations()
                .iter()
                .map(|a| AnimationSnapshot {
                    id: a.id.to_string(),
                    name: a.name.clone(),
                    fps: a.fps,
                    frames: a
                        .frames
                        .iter()
                        .map(|f| AnimationFrameSnapshot {
                            frame_id: f.frame.to_string(),
                        })
                        .collect(),
                })
                .collect(),
            palette: project
                .palette()
                .entries()
                .map(|(id, color)| PaletteEntry {
                    id: id.to_string(),
                    color: color.into(),
                    source: project.palette().source(id).expect("颜色存在").into(),
                    usage: counts.get(&id).copied().unwrap_or(0),
                })
                .collect(),
            history: HistorySnapshot {
                entries: engine.history().entries().map(str::to_owned).collect(),
                position: engine.history().position() as u32,
            },
            modified: status.modified,
            can_undo: status.can_undo,
            can_redo: status.can_redo,
            revision: status.revision.to_string(),
        }
    }
}

#[derive(Clone, Serialize, Type)]
pub struct EditorSnapshot {
    pub document: Option<DocumentSnapshot>,
    pub recent: Vec<RecentProject>,
    pub warning: Option<String>,
}

#[derive(Clone, Serialize, Type)]
pub struct FrameSnapshot {
    pub id: String,
}
#[derive(Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AnimationSnapshot {
    pub id: String,
    pub name: String,
    pub fps: f64,
    pub frames: Vec<AnimationFrameSnapshot>,
}
#[derive(Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AnimationFrameSnapshot {
    pub frame_id: String,
}
#[derive(Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CelSnapshot {
    pub frame_id: String,
    pub image_id: String,
    pub x: i32,
    pub y: i32,
}
#[derive(Clone, Serialize, Type)]
pub struct ImageSnapshot {
    pub id: String,
    pub pixels: Vec<(i32, i32, String)>,
}
#[derive(Clone, Deserialize, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum AnimationEdit {
    AddAnimation {
        name: String,
    },
    RenameAnimation {
        id: String,
        name: String,
    },
    DeleteAnimation {
        id: String,
    },
    MoveAnimation {
        id: String,
        position: u32,
    },
    NewFrame {
        after: String,
    },
    DuplicateFrame {
        frame: String,
        linked: bool,
    },
    DeleteFrame {
        frame: String,
    },
    Timing {
        animation: String,
        fps: f64,
    },
    AutoLink {
        layer: String,
        on: bool,
    },
    Link {
        layer: String,
        frames: Vec<String>,
    },
    LinkPrevious {
        layer: String,
        frame: String,
    },
    Unlink {
        layer: String,
        frames: Vec<String>,
    },
    Clear {
        layer: String,
        frames: Vec<String>,
    },
    Offset {
        layer: String,
        frame: String,
        x: i32,
        y: i32,
    },
}

#[derive(Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportPngResult {
    pub snapshot: EditorSnapshot,
    pub layer_id: Option<String>,
    pub iterations: u32,
    pub converged: bool,
    pub cancelled: bool,
}

pub use pixauro_engine::assets::{
    ImportAssembly, ImportSlice, LocateItem, LocateRequest, SliceGrid,
};
pub use pixauro_engine::{PngImportOptions, PngRaster, SpritePreview, SpriteSlice};

#[derive(Clone, Copy, Serialize, Type)]
pub struct PngInfo {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ImportStage {
    Decoding,
    Preparing,
    Optimizing,
    Frames,
    Masking,
    Placing,
    Committing,
}

#[derive(Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub stage: ImportStage,
    pub completed: u32,
    pub total: u32,
    pub colors: u16,
    pub active: u32,
}

#[derive(Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PngImportRequest {
    pub guard: DocumentGuard,
    pub path: String,
    pub frame: String,
    pub above: String,
    pub options: PngImportOptions,
    pub job_id: String,
}

#[derive(Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpritePreviewRequest {
    pub path: String,
    pub remove_source_background: bool,
    pub source_background_edge: f32,
    pub slices: Vec<SpriteSlice>,
    pub canvas_width: u16,
    pub canvas_height: u16,
    pub options: PngImportOptions,
    pub job_id: String,
}
#[derive(Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpriteImportRequest {
    pub guard: DocumentGuard,
    pub above: String,
    pub current_frame: Option<String>,
    pub name: String,
    pub preview: SpritePreview,
    pub assembly: ImportAssembly,
}
#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SpriteImportResult {
    pub snapshot: EditorSnapshot,
    pub layer_id: String,
}

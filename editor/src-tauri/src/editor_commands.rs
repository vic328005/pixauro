//! WebView 不能写项目文件、修改 Engine 或提供像素 JSON；所有变更经过这些命令。
use std::{path::PathBuf, sync::atomic::Ordering};

use pixauro_engine::{
    Engine, LayerProperties,
    commands::{
        AddPaletteColorCommand, LayerCommand, MergeLayerDownCommand, PrunePaletteCommand,
        RemovePaletteColorCommand, SetPaletteColorCommand, SetPixelsCommand,
    },
};
use tauri::{AppHandle, Manager};

use crate::{
    editor_protocol::*,
    editor_session::{AppState, Session, validate_extension, with_editor},
};

#[tauri::command]
#[specta::specta]
pub async fn snapshot(app: AppHandle) -> Result<EditorSnapshot, String> {
    with_editor(app, |editor, _| Ok(editor.snapshot())).await
}

#[tauri::command]
#[specta::specta]
pub async fn new_project(
    app: AppHandle,
    options: NewProjectOptions,
) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, app| {
        // 先校验新建参数，再确认放弃旧作品；无效输入不会诱使用户丢弃修改。
        let engine = Engine::new(options.into()).map_err(|error| error.to_string())?;
        if editor.confirm_replace(app)? {
            editor.session = Some(Session::new(engine, None));
        }
        Ok(editor.snapshot())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn open_project(
    app: AppHandle,
    path: Option<String>,
    expected_id: Option<String>,
) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, app| {
        let path = match path {
            Some(path) => PathBuf::from(path),
            None => {
                let Some(file) = editor
                    .dialog(app, "Pixauro 项目", "json")
                    .set_title("打开 Pixauro 项目")
                    .blocking_pick_file()
                else {
                    return Ok(editor.snapshot());
                };
                file.into_path().map_err(|error| error.to_string())?
            }
        };
        validate_extension(&path, "json")?;
        if !path.is_file() {
            return Err("请打开一个存在的 Pixauro JSON 文件，不支持文件夹。".into());
        }
        if !editor.confirm_replace(app)? {
            return Ok(editor.snapshot());
        }
        // 保存提示后再读取，避免重新打开当前文件时读到保存前的内容。
        let engine = Engine::load(&path).map_err(|error| error.to_string())?;
        if expected_id.is_some_and(|id| id != engine.project().id().to_string()) {
            return Err(
                "这个位置已是另一个项目。请通过打开项目重新选择，或移除这条最近记录。".into(),
            );
        }
        editor.session = Some(Session::new(engine, Some(path)));
        editor.remember();
        Ok(editor.snapshot())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn save_project(app: AppHandle, save_as: bool) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, app| {
        editor.save(app, save_as)?;
        Ok(editor.snapshot())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn close_project(app: AppHandle) -> Result<EditorSnapshot, String> {
    with_editor(app, |editor, app| {
        if editor.confirm_replace(app)? {
            editor.session = None;
        }
        Ok(editor.snapshot())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn close_window(app: AppHandle) -> Result<bool, String> {
    if app
        .state::<AppState>()
        .close_requested
        .swap(true, Ordering::SeqCst)
    {
        return Ok(false);
    }
    let result = with_editor(app.clone(), |editor, app| editor.confirm_replace(app)).await;
    app.state::<AppState>()
        .close_requested
        .store(false, Ordering::SeqCst);
    if result.as_ref().is_ok_and(|approved| *approved) {
        app.state::<AppState>()
            .close_approved
            .store(true, Ordering::SeqCst);
        app.exit(0);
    }
    result
}

#[tauri::command]
#[specta::specta]
pub async fn forget_recent(
    app: AppHandle,
    id: String,
    path: String,
) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, _| {
        editor
            .recent
            .retain(|entry| entry.id != id || entry.path != path);
        editor.persist_recent();
        Ok(editor.snapshot())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn history_jump(
    app: AppHandle,
    guard: DocumentGuard,
    position: u32,
) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, _| {
        let engine = &mut editor.guarded_session(&guard)?.engine;
        let target = position as usize;
        if target > engine.history().entries().len() {
            return Err("历史位置已失效。".into());
        }
        while engine.history().position() != target {
            let changed = if engine.history().position() > target {
                engine.undo()
            } else {
                engine.redo()
            }
            .map_err(|error| error.to_string())?;
            if !changed {
                break;
            }
        }
        Ok(editor.snapshot())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_palette_color(
    app: AppHandle,
    guard: DocumentGuard,
    id: String,
    color: RgbaColor,
) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, _| {
        let session = editor.guarded_session(&guard)?;
        let id = session.color_id(&id)?;
        session
            .engine
            .execute(SetPaletteColorCommand::new(id, color.into()))
            .map_err(|error| error.to_string())?;
        Ok(editor.snapshot())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn add_palette_color(
    app: AppHandle,
    guard: DocumentGuard,
    color: RgbaColor,
) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, _| {
        editor
            .guarded_session(&guard)?
            .engine
            .execute(AddPaletteColorCommand::new(color.into()))
            .map_err(|error| error.to_string())?;
        Ok(editor.snapshot())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn prune_palette(app: AppHandle, guard: DocumentGuard) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, _| {
        editor
            .guarded_session(&guard)?
            .engine
            .execute(PrunePaletteCommand::default())
            .map_err(|error| error.to_string())?;
        Ok(editor.snapshot())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_palette_color(
    app: AppHandle,
    guard: DocumentGuard,
    id: String,
    replacement: Option<String>,
) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, _| {
        let session = editor.guarded_session(&guard)?;
        let id = session.color_id(&id)?;
        let replacement = replacement
            .as_deref()
            .map(|id| session.color_id(id))
            .transpose()?;
        session
            .engine
            .execute(RemovePaletteColorCommand::new(id, replacement))
            .map_err(|error| error.to_string())?;
        Ok(editor.snapshot())
    })
    .await
}

/// 覆盖位图按 y * width + x 排列，每字节从最低位开始；长度只取决于画布。
/// 校验整份覆盖范围和调色板引用后，才以一条通用命令提交给唯一的 Engine。
/// 工具如何生成临时覆盖范围不属于项目模型或命令历史。
pub(crate) fn apply_stroke_coverage(
    session: &mut Session,
    layer_id: &str,
    frame_id: &str,
    color_id: Option<&str>,
    coverage: &[u8],
) -> Result<bool, String> {
    let layer_id = session.layer_id(layer_id)?;
    let frame_id = session.frame_id(frame_id)?;
    let canvas = session.engine.project().canvas();
    let width = usize::from(canvas.width());
    let pixel_count = canvas.pixel_count();
    let byte_count = pixel_count.div_ceil(8);
    if coverage.len() != byte_count {
        return Err(format!("笔画覆盖位图长度无效，应为 {byte_count} 字节。"));
    }
    let final_bits = pixel_count % 8;
    if final_bits != 0 && coverage[byte_count - 1] >> final_bits != 0 {
        return Err("笔画覆盖位图包含画布外的像素。".into());
    }
    let target = color_id.map(|id| session.color_id(id)).transpose()?;
    let covered_count = coverage.iter().map(|byte| byte.count_ones() as usize).sum();
    let mut pixels = Vec::with_capacity(covered_count);
    for (byte_index, &byte) in coverage.iter().enumerate() {
        for bit in 0..8 {
            if byte & (1 << bit) != 0 {
                let offset = byte_index * 8 + bit;
                pixels.push((((offset % width) as i32, (offset / width) as i32), target));
            }
        }
    }
    session
        .engine
        .execute(SetPixelsCommand::new(layer_id, frame_id, pixels))
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn paint_stroke(
    app: AppHandle,
    guard: DocumentGuard,
    layer_id: String,
    frame_id: String,
    color_id: Option<String>,
    coverage: Vec<u8>,
) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, _| {
        let session = editor.guarded_session(&guard)?;
        apply_stroke_coverage(
            session,
            &layer_id,
            &frame_id,
            color_id.as_deref(),
            &coverage,
        )?;
        Ok(editor.snapshot())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn export_png(
    app: AppHandle,
    guard: DocumentGuard,
    palette: bool,
    frame_id: String,
) -> Result<Option<String>, String> {
    with_editor(app, move |editor, app| {
        let name = editor
            .guarded_session(&guard)?
            .engine
            .project()
            .name()
            .to_owned();
        let suffix = if palette { "-palette" } else { "" };
        let Some(file) = editor
            .dialog(app, "PNG 图片", "png")
            .set_title("导出 PNG")
            .set_file_name(format!("{name}{suffix}.png"))
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let mut path = file.into_path().map_err(|error| error.to_string())?;
        if path.extension().is_none() {
            path.set_extension("png");
        }
        validate_extension(&path, "png")?;
        let session = editor.guarded_session(&guard)?;
        let frame = session.frame_id(&frame_id)?;
        let engine = &session.engine;
        if palette {
            engine.export_palette_png(&path)
        } else {
            engine.export_frame_png(frame, &path)
        }
        .map_err(|error| error.to_string())?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn export_sprite_sheet(
    app: AppHandle,
    guard: DocumentGuard,
    animation_id: Option<String>,
) -> Result<Option<String>, String> {
    with_editor(app, move |editor, app| {
        let session = editor.guarded_session(&guard)?;
        let animation = animation_id
            .as_deref()
            .map(|id| session.animation_id(id))
            .transpose()?;
        let Some(folder) = editor
            .dialog(app, "导出文件夹", "")
            .set_title("选择导出位置（新建文件夹，包含 PNG 和 JSON）")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let parent = folder.into_path().map_err(|error| error.to_string())?;
        let path = editor
            .guarded_session(&guard)?
            .engine
            .export_sprite_sheet(animation, parent)
            .map_err(|error| error.to_string())?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn edit_layer(
    app: AppHandle,
    guard: DocumentGuard,
    edit: LayerEdit,
) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, _| {
        let session = editor.guarded_session(&guard)?;
        let command = match edit {
            LayerEdit::Add {
                name,
                animation,
                above,
            } => {
                let animation = session.animation_id(&animation)?;
                match above {
                    Some(id) => {
                        let id = session.layer_id(&id)?;
                        if session
                            .engine
                            .project()
                            .canvas()
                            .layer(id)
                            .map_err(|e| e.to_string())?
                            .animation()
                            != animation
                        {
                            return Err("图层必须属于目标动画".into());
                        }
                        LayerCommand::add_above(id, name)
                    }
                    None => LayerCommand::add(animation, name),
                }
            }
            LayerEdit::MergeDown { id } => {
                let id = session.layer_id(&id)?;
                session
                    .engine
                    .execute(MergeLayerDownCommand::new(id))
                    .map_err(|error| error.to_string())?;
                return Ok(editor.snapshot());
            }
            LayerEdit::Duplicate { id, name } => {
                LayerCommand::duplicate(session.layer_id(&id)?, name)
            }
            LayerEdit::Remove { id } => LayerCommand::remove(session.layer_id(&id)?),
            LayerEdit::Move { id, position } => {
                LayerCommand::move_to(session.layer_id(&id)?, position as usize)
            }
            LayerEdit::Properties {
                id,
                name,
                visible,
                locked,
                opacity,
            } => LayerCommand::set_properties(
                session.layer_id(&id)?,
                LayerProperties {
                    name,
                    visible,
                    locked,
                    opacity,
                },
            ),
        };
        session
            .engine
            .execute(command)
            .map_err(|error| error.to_string())?;
        Ok(editor.snapshot())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn edit_animation(
    app: AppHandle,
    guard: DocumentGuard,
    edit: crate::editor_protocol::AnimationEdit,
) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, _| {
        use crate::editor_protocol::AnimationEdit as D;
        use pixauro_engine::commands::{AnimationCommand, AnimationEdit as E};
        let s = editor.guarded_session(&guard)?;
        let frames = |values: Vec<String>| {
            values
                .iter()
                .map(|v| s.frame_id(v))
                .collect::<Result<Vec<_>, _>>()
        };
        let edit = match edit {
            D::AddAnimation { name } => E::AddAnimation { name },
            D::RenameAnimation { id, name } => E::RenameAnimation {
                id: s.animation_id(&id)?,
                name,
            },
            D::DeleteAnimation { id } => E::DeleteAnimation {
                id: s.animation_id(&id)?,
            },
            D::MoveAnimation { id, position } => E::MoveAnimation {
                id: s.animation_id(&id)?,
                position: position as usize,
            },
            D::NewFrame { after } => E::NewFrame {
                after: s.frame_id(&after)?,
            },
            D::DuplicateFrame { frame, linked } => E::DuplicateFrame {
                frame: s.frame_id(&frame)?,
                linked,
            },
            D::DeleteFrame { frame } => E::DeleteFrame {
                frame: s.frame_id(&frame)?,
            },
            D::Timing { animation, fps } => E::Timing {
                animation: s.animation_id(&animation)?,
                fps,
            },
            D::AutoLink { layer, on } => E::AutoLink {
                layer: s.layer_id(&layer)?,
                on,
            },
            D::Link { layer, frames: f } => E::Link {
                layer: s.layer_id(&layer)?,
                frames: frames(f)?,
            },
            D::Unlink { layer, frames: f } => E::Unlink {
                layer: s.layer_id(&layer)?,
                frames: frames(f)?,
            },
            D::Clear { layer, frames: f } => E::Clear {
                layer: s.layer_id(&layer)?,
                frames: frames(f)?,
            },
            D::LinkPrevious { layer, frame } => E::LinkPrevious {
                layer: s.layer_id(&layer)?,
                frame: s.frame_id(&frame)?,
            },
            D::Offset { layer, frame, x, y } => E::Offset {
                layer: s.layer_id(&layer)?,
                frame: s.frame_id(&frame)?,
                x,
                y,
            },
        };
        s.engine
            .execute(AnimationCommand::new(edit))
            .map_err(|e| e.to_string())?;
        Ok(editor.snapshot())
    })
    .await
}

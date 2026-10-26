//! 首页只按需读取保存到磁盘的作品，不替换当前编辑会话。
use std::{path::Path, process::Command, time::UNIX_EPOCH};

use pixauro_engine::Engine;
use tauri::AppHandle;

use crate::{
    editor_protocol::{EditorSnapshot, ProjectPreview, RecentProject},
    editor_session::{Editor, with_editor},
};

fn load_recent(editor: &Editor, id: &str, path: &str) -> Result<Engine, String> {
    if !editor
        .recent
        .iter()
        .any(|entry| entry.id == id && entry.path == path)
    {
        return Err("项目已不在最近列表中，请刷新首页。".into());
    }
    let engine = Engine::load(path).map_err(|error| error.to_string())?;
    if engine.project().id().to_string() != id {
        return Err("此路径已被其他项目替换，请重新打开文件。".into());
    }
    Ok(engine)
}

#[tauri::command]
#[specta::specta]
pub async fn project_preview(
    app: AppHandle,
    id: String,
    path: String,
) -> Result<ProjectPreview, String> {
    with_editor(app, move |editor, _| {
        let engine = load_recent(editor, &id, &path)?;
        let project = engine.project();
        let canvas = project.canvas();
        let width = canvas.width();
        let height = canvas.height();
        // IPC 预览最多 192×192；大作品用最近邻缩小，原始尺寸单独返回。
        let ratio = (192.0 / f64::from(width.max(height))).min(1.0);
        let preview_width = (f64::from(width) * ratio).round().max(1.0) as u16;
        let preview_height = (f64::from(height) * ratio).round().max(1.0) as u16;
        let mut rgba =
            Vec::with_capacity(usize::from(preview_width) * usize::from(preview_height) * 4);
        for y in 0..u32::from(preview_height) {
            for x in 0..u32::from(preview_width) {
                let source_x = x * u32::from(width) / u32::from(preview_width);
                let source_y = y * u32::from(height) / u32::from(preview_height);
                let color = project
                    .color_at((source_x as i32, source_y as i32))
                    .map_err(|error| error.to_string())?;
                rgba.extend_from_slice(&[color.red, color.green, color.blue, color.alpha]);
            }
        }
        let updated_at = std::fs::metadata(&path)
            .and_then(|data| data.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|time| time.as_secs_f64() * 1000.0);
        Ok(ProjectPreview {
            name: project.name().into(),
            width,
            height,
            updated_at,
            preview_width,
            preview_height,
            rgba,
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn duplicate_recent(
    app: AppHandle,
    id: String,
    path: String,
) -> Result<EditorSnapshot, String> {
    with_editor(app, move |editor, _| {
        let engine = load_recent(editor, &id, &path)?;
        let source = Path::new(&path);
        let parent = source.parent().ok_or("项目路径无效")?;
        let stem: String = source
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or("文件名无效")?
            .chars()
            .take(110)
            .collect();
        for index in 1..=1000 {
            let name = if index == 1 {
                format!("{stem} copy")
            } else {
                format!("{stem} copy {index}")
            };
            let destination = parent.join(format!("{name}.json"));
            if destination
                .try_exists()
                .map_err(|error| error.to_string())?
            {
                continue;
            }
            engine
                .duplicate_to(&destination, &name)
                .map_err(|error| error.to_string())?;
            let duplicate = Engine::load(&destination).map_err(|error| error.to_string())?;
            editor.remember_project(RecentProject {
                id: duplicate.project().id().to_string(),
                name,
                path: destination.to_string_lossy().into_owned(),
            });
            return Ok(editor.snapshot());
        }
        Err("副本名称已用尽，请先重命名已有副本。".into())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn reveal_recent(app: AppHandle, id: String, path: String) -> Result<(), String> {
    with_editor(app, move |editor, _| {
        if !editor
            .recent
            .iter()
            .any(|entry| entry.id == id && entry.path == path)
        {
            return Err("项目已不在最近列表中。".into());
        }
        if !Path::new(&path).is_file() {
            return Err("项目文件不存在或已被移动。".into());
        }
        #[cfg(target_os = "macos")]
        let status = Command::new("open").arg("-R").arg(&path).status();
        #[cfg(target_os = "windows")]
        let status = Command::new("explorer.exe")
            .arg(format!("/select,{path}"))
            .status();
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let status = Command::new("xdg-open")
            .arg(Path::new(&path).parent().ok_or("项目路径无效")?)
            .status();
        if !status.map_err(|error| error.to_string())?.success() {
            return Err("无法在文件管理器中显示项目。".into());
        }
        Ok(())
    })
    .await
}

//! Tauri 原生宿主。React 只持有服务端快照及临时工具状态；文档属于 Engine。
mod editor_commands;
pub mod editor_protocol;
mod editor_session;
mod gallery;
mod sprite_import;
#[cfg(test)]
mod tools;
mod window_state;

#[cfg(test)]
mod migration_tests;

use std::{path::Path, sync::atomic::Ordering};
use tauri::{Emitter, Manager};

use editor_session::AppState;

/// 同一份命令注册同时生成调用处理器和 TypeScript，避免手写边界漂移。
pub fn bindings() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new().commands(tauri_specta::collect_commands![
        editor_commands::snapshot,
        editor_commands::new_project,
        editor_commands::open_project,
        editor_commands::save_project,
        editor_commands::close_project,
        editor_commands::close_window,
        editor_commands::forget_recent,
        gallery::project_preview,
        gallery::duplicate_recent,
        gallery::reveal_recent,
        editor_commands::history_jump,
        editor_commands::set_palette_color,
        editor_commands::add_palette_color,
        editor_commands::remove_palette_color,
        editor_commands::prune_palette,
        editor_commands::paint_stroke,
        editor_commands::edit_layer,
        editor_commands::edit_animation,
        editor_commands::export_png,
        editor_commands::export_sprite_sheet,
        sprite_import::grid_slices,
        sprite_import::auto_locate_slices,
        sprite_import::png_source_preview,
        sprite_import::prepare_sprite_preview,
        sprite_import::commit_sprite_import,
        sprite_import::inspect_png,
        sprite_import::import_png,
        sprite_import::png_import_status,
        sprite_import::cancel_png_import,
    ])
}

pub fn export_bindings(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    bindings().export(specta_typescript::Typescript::default(), path)?;
    Ok(())
}

fn request_close(app: &tauri::AppHandle) {
    if let Err(error) = app.emit("editor-close-requested", ()) {
        eprintln!("无法通知编辑器完成当前笔画：{error}");
    }
}

pub fn run() {
    let bindings = bindings();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(window_state::plugin())
        .invoke_handler(bindings.invoke_handler())
        .setup(move |app| {
            let path = app.path().app_config_dir()?.join("recent-projects.json");
            app.manage(AppState::new(path));
            bindings.mount_events(app);
            window_state::setup(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            window_state::on_window_event(window, event);
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                if !app
                    .state::<AppState>()
                    .close_approved
                    .load(Ordering::SeqCst)
                {
                    api.prevent_close();
                    // 先让前端提交尚未收笔的覆盖范围，再由 close_window 执行原生保存确认。
                    request_close(app);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("Pixauro 原生窗口启动失败");
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event
            && !app
                .state::<AppState>()
                .close_approved
                .load(Ordering::SeqCst)
        {
            api.prevent_exit();
            request_close(app);
        }
    });
}

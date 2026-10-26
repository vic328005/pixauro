//! 窗口习惯属于应用配置，不进入作品文件或撤销历史。
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::mpsc::{self, RecvTimeoutError, SyncSender},
    time::Duration,
};
use tauri::Manager;
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

struct Autosave(SyncSender<()>);

fn flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED | StateFlags::FULLSCREEN
}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_window_state::Builder::default()
        .with_state_flags(flags())
        .with_filter(|label| label == "main")
        // setup 直接以保存的尺寸创建窗口，插件只负责后续记录，不再二次缩放。
        .skip_initial_state("main")
        .build()
}

pub fn setup(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .ok_or("缺少主窗口配置")?
        .clone();
    if let Err(error) = apply_saved_geometry(app, &mut config) {
        eprintln!("无法读取窗口状态，使用默认尺寸：{error}");
    }
    // 创建参数决定首帧尺寸；不能在 build 后调用 set_size/restore_state 再 show。
    let window = tauri::WebviewWindowBuilder::from_config(app, &config)?
        .visible(false)
        .build()?;
    window.show()?;

    let (sender, receiver) = mpsc::sync_channel(1);
    let handle = app.handle().clone();
    std::thread::Builder::new()
        .name("window-state-autosave".into())
        .spawn(move || {
            while receiver.recv().is_ok() {
                // 拖拽过程合并事件；停止变化 250 ms 后保存最新状态。
                loop {
                    match receiver.recv_timeout(Duration::from_millis(250)) {
                        Ok(()) => continue,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                let app = handle.clone();
                // 插件缓存与原生窗口查询都在主线程执行，避免与 resize 回调争锁。
                if let Err(error) = handle.run_on_main_thread(move || {
                    if let Err(error) = app.save_window_state(flags()) {
                        eprintln!("无法保存窗口状态：{error}");
                    }
                }) {
                    eprintln!("无法调度窗口状态保存：{error}");
                }
            }
        })?;
    app.manage(Autosave(sender));
    Ok(())
}

pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() == "main"
        && matches!(
            event,
            tauri::WindowEvent::Resized(_)
                | tauri::WindowEvent::Moved(_)
                | tauri::WindowEvent::ScaleFactorChanged { .. }
        )
        && let Some(autosave) = window.app_handle().try_state::<Autosave>()
    {
        // 满队列已有一次待处理变化，无需阻塞窗口线程。
        let _ = autosave.0.try_send(());
    }
}

/// 读取 window-state 插件当前保存格式中的几何字段；其余字段由插件管理。
#[derive(Deserialize)]
struct SavedGeometry {
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    prev_x: i32,
    prev_y: i32,
    maximized: bool,
    fullscreen: bool,
}

fn apply_saved_geometry(
    app: &tauri::App,
    config: &mut tauri::utils::config::WindowConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = app.path().app_config_dir()?.join(app.handle().filename());
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let saved: HashMap<String, SavedGeometry> = serde_json::from_reader(file)?;
    let Some(saved) = saved.get("main").filter(|s| s.width > 0 && s.height > 0) else {
        return Ok(());
    };
    let (x, y) = if saved.maximized {
        (saved.prev_x, saved.prev_y)
    } else {
        (saved.x, saved.y)
    };
    let monitors = app.available_monitors()?;
    let original_monitor = monitors.iter().find(|monitor| {
        let position = monitor.position();
        let size = monitor.size();
        i64::from(x) >= i64::from(position.x)
            && i64::from(y) >= i64::from(position.y)
            && i64::from(x) < i64::from(position.x) + i64::from(size.width)
            && i64::from(y) < i64::from(position.y) + i64::from(size.height)
    });
    let primary = app.primary_monitor()?;
    let Some(monitor) = original_monitor.or(primary.as_ref()).or(monitors.first()) else {
        return Ok(());
    };
    // 插件保存物理像素，WindowConfig 接收逻辑像素；先按目标屏幕缩放比例换算。
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    config.width =
        (f64::from(saved.width.min(area.size.width)) / scale).max(config.min_width.unwrap_or(0.0));
    config.height = (f64::from(saved.height.min(area.size.height)) / scale)
        .max(config.min_height.unwrap_or(0.0));
    if original_monitor.is_some() {
        config.x = Some(f64::from(x) / scale);
        config.y = Some(f64::from(y) / scale);
    } else {
        // 原显示器断开时不把窗口放到屏幕之外。
        config.x = None;
        config.y = None;
        config.center = true;
    }
    config.maximized = saved.maximized;
    config.fullscreen = saved.fullscreen;
    Ok(())
}

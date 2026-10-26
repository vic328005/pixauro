//! 桌面会话、最近项目和文件流程；Engine 是唯一正式文档所有者。
//! Mutex 的加锁与原生阻塞对话框只在 spawn_blocking 工作线程执行。

use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::AtomicBool},
};

use pixauro_engine::{AnimationId, ColorId, Engine, FrameId, LayerId, ProjectId};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{
    DialogExt, FileDialogBuilder, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

use crate::editor_protocol::{DocumentGuard, DocumentSnapshot, EditorSnapshot, RecentProject};

pub(crate) struct Session {
    pub engine: Engine,
    pub path: Option<PathBuf>,
    // 即使重新打开同一个 UUID，也必须拒绝上一工作区的异步笔画。
    pub id: String,
}

impl Session {
    pub fn new(engine: Engine, path: Option<PathBuf>) -> Self {
        Self {
            engine,
            path,
            id: ProjectId::new_v4().to_string(),
        }
    }

    pub fn animation_id(&self, value: &str) -> Result<AnimationId, String> {
        self.engine
            .project()
            .canvas()
            .animations()
            .iter()
            .find(|a| a.id.to_string() == value)
            .map(|a| a.id)
            .ok_or_else(|| format!("动画 {value} 不存在"))
    }
    pub fn frame_id(&self, value: &str) -> Result<FrameId, String> {
        self.engine
            .project()
            .canvas()
            .frames()
            .iter()
            .find(|f| f.id.to_string() == value)
            .map(|f| f.id)
            .ok_or_else(|| format!("帧 {value} 不存在"))
    }
    pub fn layer_id(&self, value: &str) -> Result<LayerId, String> {
        self.engine
            .project()
            .canvas()
            .layers()
            .iter()
            .find_map(|layer| (layer.id().to_string() == value).then_some(layer.id()))
            .ok_or_else(|| format!("不存在图层 ID {value}"))
    }

    pub fn color_id(&self, value: &str) -> Result<ColorId, String> {
        // 从现有调色板解析，既不暴露引擎内部构造器，也不经浮点数转换。
        self.engine
            .project()
            .palette()
            .entries()
            .find_map(|(id, _)| (id.to_string() == value).then_some(id))
            .ok_or_else(|| format!("调色板中不存在颜色 ID {value}"))
    }
}

pub(crate) struct Editor {
    pub session: Option<Session>,
    pub recent: Vec<RecentProject>,
    recent_path: PathBuf,
    warning: Option<String>,
}

pub struct AppState {
    pub(crate) editor: Arc<Mutex<Editor>>,
    pub(crate) import_job: Arc<Mutex<Option<crate::sprite_import::ImportJob>>>,
    pub(crate) close_approved: AtomicBool,
    pub(crate) close_requested: AtomicBool,
}

impl AppState {
    pub(crate) fn new(recent_path: PathBuf) -> Self {
        let (entries, warning) = match std::fs::read(&recent_path) {
            Ok(bytes) => match serde_json::from_slice::<Vec<RecentProject>>(&bytes) {
                Ok(entries) => (entries, None),
                Err(error) => (Vec::new(), Some(format!("最近项目记录读取失败：{error}"))),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Vec::new(), None),
            Err(error) => (Vec::new(), Some(format!("最近项目记录读取失败：{error}"))),
        };
        let mut recent: Vec<RecentProject> = Vec::new();
        for entry in entries {
            if ProjectId::parse_str(&entry.id).is_ok_and(|id| !id.is_nil())
                && Path::new(&entry.path).is_absolute()
                && !recent
                    .iter()
                    .any(|old| old.id == entry.id || old.path == entry.path)
            {
                recent.push(entry);
            }
            if recent.len() == 12 {
                break;
            }
        }
        Self {
            editor: Arc::new(Mutex::new(Editor {
                session: None,
                recent,
                recent_path,
                warning,
            })),
            import_job: Arc::new(Mutex::new(None)),
            close_approved: AtomicBool::new(false),
            close_requested: AtomicBool::new(false),
        }
    }
}

impl Editor {
    pub fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            document: self.session.as_ref().map(|session| {
                DocumentSnapshot::new(&session.engine, &session.id, session.path.as_deref())
            }),
            recent: self.recent.clone(),
            warning: self.warning.clone(),
        }
    }

    pub fn guarded_session(&mut self, guard: &DocumentGuard) -> Result<&mut Session, String> {
        let session = self.session.as_mut().ok_or("当前没有打开的项目")?;
        if session.id != guard.session_id {
            return Err("项目已切换，已拒绝旧工作区的操作。".into());
        }
        if session.engine.status().revision.to_string() != guard.revision {
            return Err("项目内容已变化，已拒绝过期操作，请重试。".into());
        }
        Ok(session)
    }

    pub fn remember(&mut self) {
        let Some(session) = &self.session else { return };
        let Some(path) = &session.path else { return };
        let Ok(path) = path.canonicalize().or_else(|_| std::path::absolute(path)) else {
            return;
        };
        let project = session.engine.project();
        let id = project.id().to_string();
        let name = project.name().to_owned();
        let path = path.to_string_lossy().into_owned();
        self.remember_project(RecentProject { id, name, path });
    }

    pub fn remember_project(&mut self, project: RecentProject) {
        self.recent
            .retain(|entry| entry.id != project.id && entry.path != project.path);
        self.recent.insert(0, project);
        self.recent.truncate(12);
        self.persist_recent();
    }

    pub fn persist_recent(&mut self) {
        // 最近列表失败不把已成功的项目保存报告为失败，单独返回可见警告。
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let parent = self.recent_path.parent().ok_or("最近列表路径无效")?;
            std::fs::create_dir_all(parent)?;
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            serde_json::to_writer_pretty(temporary.as_file_mut(), &self.recent)?;
            temporary.as_file_mut().flush()?;
            temporary.as_file().sync_all()?;
            temporary.persist(&self.recent_path)?;
            Ok(())
        })();
        self.warning = result
            .err()
            .map(|error| format!("项目操作已完成，但最近项目列表未能保存：{error}"));
    }

    pub fn dialog(
        &self,
        app: &AppHandle,
        label: &str,
        extension: &str,
    ) -> FileDialogBuilder<tauri::Wry> {
        let mut dialog = app.dialog().file();
        if !extension.is_empty() {
            dialog = dialog.add_filter(label, &[extension]);
        }
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        if let Some(parent) = self
            .session
            .as_ref()
            .and_then(|session| session.path.as_deref())
            .and_then(Path::parent)
        {
            dialog = dialog.set_directory(parent);
        }
        dialog
    }

    /// 取消或失败不会清除脏标记、路径、文档、历史，也不会继续关闭/替换。
    pub fn save(&mut self, app: &AppHandle, save_as: bool) -> Result<bool, String> {
        let session = self.session.as_ref().ok_or("当前没有打开的项目")?;
        let choose_path = save_as || session.path.is_none();
        let path = if choose_path {
            let name = session
                .path
                .as_deref()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("{}.json", session.engine.project().name()));
            let Some(file) = self
                .dialog(app, "Pixauro 项目", "json")
                .set_title("保存 Pixauro 项目")
                .set_file_name(name)
                .blocking_save_file()
            else {
                return Ok(false);
            };
            let mut path = file.into_path().map_err(|error| error.to_string())?;
            if path.extension().is_none() {
                path.set_extension("json");
            }
            validate_extension(&path, "json")?;
            path
        } else {
            session.path.clone().ok_or("项目保存路径不存在")?
        };
        let session = self.session.as_mut().ok_or("当前没有打开的项目")?;
        if choose_path {
            let name = path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or("文件名必须是有效文本")?;
            session
                .engine
                .save_as(&path, name)
                .map_err(|error| error.to_string())?;
        } else {
            session
                .engine
                .save(&path)
                .map_err(|error| error.to_string())?;
        }
        session.path = Some(path);
        self.remember();
        Ok(true)
    }

    pub fn confirm_replace(&mut self, app: &AppHandle) -> Result<bool, String> {
        let Some(session) = &self.session else {
            return Ok(true);
        };
        if !session.engine.status().modified {
            return Ok(true);
        }
        let mut dialog = app
            .dialog()
            .message(format!(
                "“{}”有未保存的修改。是否先保存？",
                session.engine.project().name()
            ))
            .title("项目尚未保存")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::YesNoCancelCustom(
                "保存".into(),
                "不保存".into(),
                "取消".into(),
            ));
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.parent(&window);
        }
        match dialog.blocking_show_with_result() {
            MessageDialogResult::Yes => self.save(app, false),
            MessageDialogResult::No => Ok(true),
            MessageDialogResult::Custom(label) if label == "保存" => self.save(app, false),
            MessageDialogResult::Custom(label) if label == "不保存" => Ok(true),
            _ => Ok(false),
        }
    }

    pub fn refresh_title(&self, app: &AppHandle) {
        let title = match &self.session {
            Some(session) => format!("{} — Pixauro", session.engine.project().name()),
            None => "Pixauro — 首页".into(),
        };
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_title(&title);
        }
    }
}

pub(crate) fn validate_extension(path: &Path, extension: &str) -> Result<(), String> {
    if path
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
    {
        Ok(())
    } else {
        Err(format!("请选择 .{extension} 文件。"))
    }
}

pub(crate) async fn with_editor<T: Send + 'static>(
    app: AppHandle,
    operation: impl FnOnce(&mut Editor, &AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let editor = app.state::<AppState>().editor.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut editor = editor
            .lock()
            .map_err(|_| "编辑器状态不可用，请重新启动。".to_owned())?;
        let result = operation(&mut editor, &app);
        editor.refresh_title(&app);
        result
    })
    .await
    .map_err(|error| format!("桌面工作线程失败：{error}"))?
}

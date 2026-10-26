//! 导入任务独立于编辑器锁：耗时求解、进度与取消均不阻塞文档访问。
use crate::{
    editor_protocol::*,
    editor_session::{AppState, validate_extension, with_editor},
};
use std::{
    ops::ControlFlow,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{AppHandle, Manager};

pub(crate) struct ImportJob {
    id: String,
    cancel: Arc<AtomicBool>,
    progress: ImportProgress,
}
struct ImportLease {
    id: String,
    jobs: Arc<Mutex<Option<ImportJob>>>,
    cancel: Arc<AtomicBool>,
}
impl Drop for ImportLease {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.jobs.lock()
            && jobs.as_ref().is_some_and(|job| job.id == self.id)
        {
            *jobs = None;
        }
    }
}
fn initial_progress() -> ImportProgress {
    ImportProgress {
        stage: ImportStage::Decoding,
        completed: 0,
        total: 1,
        colors: 0,
        active: 0,
    }
}
impl ImportLease {
    fn reserve(app: &AppHandle, id: String) -> Result<Self, String> {
        if id.is_empty() || id.len() > 128 {
            return Err("导入任务 ID 无效".into());
        }
        let jobs = app.state::<AppState>().import_job.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut slot = jobs.lock().map_err(|_| "导入状态不可用")?;
            if slot.is_some() {
                return Err("已有图片正在导入，请等待或取消".into());
            }
            *slot = Some(ImportJob {
                id: id.clone(),
                cancel: cancel.clone(),
                progress: initial_progress(),
            });
        }
        Ok(Self { id, jobs, cancel })
    }
    fn report_frames(&self, p: pixauro_engine::SpriteImportProgress) -> ControlFlow<()> {
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(job) = jobs.as_mut()
        {
            job.progress = ImportProgress {
                stage: if p.completed == 0 && p.active == 0 {
                    ImportStage::Decoding
                } else {
                    ImportStage::Frames
                },
                completed: p.completed,
                total: p.total,
                active: p.active,
                colors: 0,
            };
        }
        if self.cancel.load(Ordering::SeqCst) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
    fn report(&self, p: pixauro_engine::PngImportProgress) -> ControlFlow<()> {
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(job) = jobs.as_mut()
        {
            use pixauro_engine::PngImportStage;
            job.progress = ImportProgress {
                stage: match p.stage {
                    PngImportStage::Decoding => ImportStage::Decoding,
                    PngImportStage::Preparing => ImportStage::Preparing,
                    PngImportStage::Optimizing => ImportStage::Optimizing,
                    PngImportStage::Masking => ImportStage::Masking,
                    PngImportStage::Placing => ImportStage::Placing,
                },
                completed: p.completed,
                total: p.total,
                colors: p.colors,
                active: 0,
            };
        }
        if self.cancel.load(Ordering::SeqCst) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn inspect_png(path: String) -> Result<PngInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(path);
        validate_extension(&path, "png")?;
        let info = pixauro_engine::inspect_png(path).map_err(|e| e.to_string())?;
        Ok(PngInfo {
            width: info.width,
            height: info.height,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn png_import_status(
    app: AppHandle,
    job_id: String,
) -> Result<Option<ImportProgress>, String> {
    let state = app.state::<AppState>();
    let jobs = state.import_job.lock().map_err(|_| "导入状态不可用")?;
    Ok(jobs
        .as_ref()
        .filter(|job| job.id == job_id)
        .map(|job| job.progress.clone()))
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_png_import(app: AppHandle, job_id: String) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let jobs = state.import_job.lock().map_err(|_| "导入状态不可用")?;
    if let Some(job) = jobs
        .as_ref()
        .filter(|job| job.id == job_id && job.progress.stage != ImportStage::Committing)
    {
        job.cancel.store(true, Ordering::SeqCst);
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
#[specta::specta]
pub async fn import_png(
    app: AppHandle,
    request: PngImportRequest,
) -> Result<ImportPngResult, String> {
    let PngImportRequest {
        guard,
        path,
        frame,
        above,
        options,
        job_id,
    } = request;
    let lease = ImportLease::reserve(&app, job_id)?;
    let checked_guard = guard.clone();
    let (size, frame, above) = with_editor(app.clone(), move |editor, _| {
        let session = editor.guarded_session(&checked_guard)?;
        let canvas = session.engine.project().canvas();
        if canvas.layers().len() >= pixauro_engine::Canvas::MAX_LAYERS {
            return Err("最多支持 256 个图层".into());
        }
        Ok((
            (canvas.width(), canvas.height()),
            session.frame_id(&frame)?,
            session.layer_id(&above)?,
        ))
    })
    .await?;
    let (lease, prepared) = tauri::async_runtime::spawn_blocking(move || {
        let prepared = (|| {
            let path = PathBuf::from(path);
            validate_extension(&path, "png")?;
            pixauro_engine::prepare_png_import(path, size, options, |p| lease.report(p))
                .map_err(|e| e.to_string())
        })();
        (lease, prepared)
    })
    .await
    .map_err(|e| format!("图片处理失败：{e}"))?;
    let prepared = prepared?;
    with_editor(app, move |editor, _| {
        // 与取消共享此锁，明确提交边界：取消成功就绝不创建图层。
        let mut jobs = lease.jobs.lock().map_err(|_| "导入状态不可用")?;
        if lease.cancel.load(Ordering::SeqCst) || prepared.is_none() {
            return Ok(ImportPngResult {
                snapshot: editor.snapshot(),
                layer_id: None,
                iterations: 0,
                converged: false,
                cancelled: true,
            });
        }
        if let Some(job) = jobs.as_mut() {
            job.progress.stage = ImportStage::Committing;
        }
        let prepared = prepared.expect("已检查结果");
        let (iterations, converged) = (prepared.iterations, prepared.converged);
        let session = editor.guarded_session(&guard)?;
        let layer = session
            .engine
            .apply_png_import(prepared, frame, above)
            .map_err(|e| e.to_string())?;
        Ok(ImportPngResult {
            snapshot: editor.snapshot(),
            layer_id: Some(layer.to_string()),
            iterations,
            converged,
            cancelled: false,
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn png_source_preview(
    path: String,
    remove_source_background: bool,
    source_background_edge: f32,
) -> Result<PngRaster, String> {
    tauri::async_runtime::spawn_blocking(move || {
        validate_extension(std::path::Path::new(&path), "png")?;
        pixauro_engine::png_source_preview(path, remove_source_background, source_background_edge)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn prepare_sprite_preview(
    app: AppHandle,
    request: SpritePreviewRequest,
) -> Result<Option<SpritePreview>, String> {
    let lease = ImportLease::reserve(&app, request.job_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        validate_extension(std::path::Path::new(&request.path), "png")?;
        let slices = request.slices;
        let prepared = pixauro_engine::prepare_sprite_preview(
            request.path,
            &slices,
            (request.canvas_width, request.canvas_height),
            request.options,
            request.remove_source_background,
            request.source_background_edge,
            |p| lease.report_frames(p),
        )
        .map_err(|e| e.to_string())?;
        if lease.cancel.load(Ordering::SeqCst) {
            return Ok(None);
        }
        Ok(prepared)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn commit_sprite_import(
    app: AppHandle,
    request: SpriteImportRequest,
) -> Result<SpriteImportResult, String> {
    with_editor(app, move |editor, _| {
        let session = editor.guarded_session(&request.guard)?;
        let above = session.layer_id(&request.above)?;
        let frame = request
            .current_frame
            .as_deref()
            .map(|id| session.frame_id(id))
            .transpose()?;
        let canvas = session.engine.project().canvas();
        let animations = pixauro_engine::assets::assemble_sprite_import(
            request.preview,
            &request.assembly,
            (canvas.width(), canvas.height()),
        )
        .map_err(|e| e.to_string())?;
        let layer = session
            .engine
            .apply_sprite_import(above, frame, request.name, animations)
            .map_err(|e| e.to_string())?;
        Ok(SpriteImportResult {
            snapshot: editor.snapshot(),
            layer_id: layer.to_string(),
        })
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub fn grid_slices(width: u32, height: u32, grid: SliceGrid) -> Result<Vec<ImportSlice>, String> {
    pixauro_engine::assets::grid_slices(width, height, grid).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn auto_locate_slices(
    app: AppHandle,
    job_id: String,
    request: LocateRequest,
) -> Result<Option<Vec<LocateItem>>, String> {
    let lease = ImportLease::reserve(&app, job_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let result = pixauro_engine::assets::auto_locate_slices(&request, |_| {
            if lease.cancel.load(Ordering::SeqCst) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .map_err(|e| e.to_string())?;
        if lease.cancel.load(Ordering::SeqCst) {
            return Ok(None);
        }
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

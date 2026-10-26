//! Engine 的完整项目读写：作品与操作栈作为一个整体原子保存。
//! 显示缓存版本、路径和工具状态都不进入文件。

use std::fs::File;
use std::io::Read;
use std::path::Path;

use super::atomic_file::{WriteMode, write_bytes};
use crate::engine::CommandHistory;
use crate::model::{CanvasFile, PaletteFile};
use crate::{Canvas, Palette, ProjectId};
use crate::{EditError, Engine, Project, ProjectError};
use serde::{Deserialize, Serialize};

// 文件格式与大小限制属于内部实现，不作为引擎公共接口暴露。
const FORMAT_VERSION: u32 = 18;
const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

impl Engine {
    /// 保存正式作品、完整操作栈和当前游标。工具预览由调用方管理。
    fn to_json(&self) -> Result<String, EditError> {
        self.json_with_project(&self.project)
    }

    fn json_with_project(&self, project: &Project) -> Result<String, EditError> {
        let data = ProjectFile {
            version: FORMAT_VERSION,
            id: project.id(),
            name: project.name().to_owned(),
            palette: project.palette().to_file(),
            canvas: project.canvas().to_file(),
            history: &self.history,
        };
        let json = serde_json::to_string_pretty(&data).map_err(ProjectError::from)?;
        Self::validate_file_size(json.len())?;
        Ok(json)
    }

    /// 作品和历史的基本有效性检查通过后才返回新的 Engine，不会修改已有实例。
    fn from_json(json: &str) -> Result<Self, EditError> {
        Self::validate_file_size(json.len())?;
        #[derive(Deserialize)]
        struct Header {
            version: u32,
        }
        let header: Header = serde_json::from_str(json).map_err(ProjectError::from)?;
        if header.version != FORMAT_VERSION {
            return Err(ProjectError::InvalidData(format!(
                "不支持项目格式版本 {}，当前仅支持版本 {}",
                header.version, FORMAT_VERSION
            ))
            .into());
        }
        let data: ProjectFile<CommandHistory> =
            serde_json::from_str(json).map_err(ProjectError::from)?;
        let palette = Palette::from_file(data.palette)?;
        let canvas = Canvas::from_file(data.canvas, &palette)?;
        let project = Project::restore(data.id, &data.name, palette, canvas)?;
        let mut history = data.history;
        history.restore(&project)?;
        Ok(Self::from_parts(project, history))
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, EditError> {
        let mut json = String::new();
        File::open(path)
            .map_err(ProjectError::from)?
            .take(MAX_FILE_BYTES as u64 + 1)
            .read_to_string(&mut json)
            .map_err(ProjectError::from)?;
        Self::from_json(&json)
    }

    pub fn save(&mut self, path: impl AsRef<Path>) -> Result<(), EditError> {
        let json = self.to_json()?;
        write_bytes(path.as_ref(), json.as_bytes(), WriteMode::Replace)?;
        self.mark_saved();
        Ok(())
    }

    /// 复制作品及完整历史，分配新的项目身份；目标必须不存在。
    pub fn duplicate_to(&self, path: impl AsRef<Path>, name: &str) -> Result<(), EditError> {
        let project = self.project.duplicate(name)?;
        let json = self.json_with_project(&project)?;
        write_bytes(path.as_ref(), json.as_bytes(), WriteMode::CreateNew)?;
        Ok(())
    }

    /// 带名称的另存为：先在候选作品上改名，文件写入成功后再同步当前名称。
    /// 失败时名称、UUID、操作栈和游标全部保持原样；改名仍不产生编辑命令。
    /// 路径与名称分开传入，Engine 不依赖桌面“名称取文件名”的交互约定。
    pub fn save_as(
        &mut self,
        path: impl AsRef<Path>,
        name: impl Into<String>,
    ) -> Result<(), EditError> {
        let mut candidate = self.project.clone();
        candidate.rename(name)?;
        let json = self.json_with_project(&candidate)?;
        write_bytes(path.as_ref(), json.as_bytes(), WriteMode::Replace)?;
        self.rename(candidate.name())?;
        self.mark_saved();
        Ok(())
    }

    fn validate_file_size(bytes: usize) -> Result<(), ProjectError> {
        if bytes > MAX_FILE_BYTES {
            return Err(ProjectError::InvalidData(
                "项目 JSON（含历史）不能超过 64 MiB".into(),
            ));
        }
        Ok(())
    }
}

// 文件封装只在 IO 层使用；写入借用历史，读取持有历史，避免克隆命令栈。
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectFile<H> {
    version: u32,
    id: ProjectId,
    name: String,
    palette: PaletteFile,
    canvas: CanvasFile,
    history: H,
}

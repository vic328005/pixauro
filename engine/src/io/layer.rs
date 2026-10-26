//! 自包含图层 JSON 导出，不包含项目历史。
use super::atomic_file::{WriteMode, write_bytes};
use crate::{EditError, Engine, LayerId, ProjectError};
use std::path::Path;

impl Engine {
    /// 自包含图层数据：原始索引、不透明度、可见性、尺寸及使用的调色板条目。
    /// 不含历史，不是完整项目文件；导出不改变保存状态。
    pub fn export_layer_data(&self, id: LayerId, path: impl AsRef<Path>) -> Result<(), EditError> {
        let project = self.project();
        let layer = project.canvas().layer(id)?;
        let used: std::collections::HashSet<_> = layer
            .images()
            .values()
            .flat_map(|p| p.iter().map(|(_, id)| *id))
            .collect();
        let palette: Vec<_> = project
            .palette()
            .entries()
            .filter(|(id, _)| used.contains(id))
            .map(
                |(id, c)| serde_json::json!({"id": id, "color": [c.red, c.green, c.blue, c.alpha], "source": project.palette().source(id)}),
            )
            .collect();
        let data = serde_json::json!({
            "format": "pixauro-layer", "version": 5, "project_id": project.id(),
            "width": project.canvas().width(), "height": project.canvas().height(),
            "layer": layer, "palette": palette,
        });
        let json = serde_json::to_string_pretty(&data).map_err(ProjectError::from)?;
        write_bytes(path.as_ref(), json.as_bytes(), WriteMode::Replace)?;
        Ok(())
    }
}

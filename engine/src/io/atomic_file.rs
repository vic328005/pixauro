//! 同目录临时写入并同步，全部内容成功后才提交目标文件。
use crate::ProjectError;
use std::{fs::File, io::Write, path::Path};

pub(super) enum WriteMode {
    Replace,
    CreateNew,
}

pub(super) fn write_bytes(path: &Path, bytes: &[u8], mode: WriteMode) -> Result<(), ProjectError> {
    write_atomic(path, mode, |file| {
        file.write_all(bytes)?;
        Ok(())
    })
}

pub(super) fn write_atomic(
    path: &Path,
    mode: WriteMode,
    write: impl FnOnce(&mut File) -> Result<(), ProjectError>,
) -> Result<(), ProjectError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    write(temporary.as_file_mut())?;
    temporary.as_file().sync_all()?;
    match mode {
        WriteMode::Replace => temporary.persist(path),
        WriteMode::CreateNew => temporary.persist_noclobber(path),
    }
    .map_err(|error| ProjectError::Io(error.error))?;
    Ok(())
}

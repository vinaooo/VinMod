use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::Path;

#[derive(Debug)]
pub enum FileSystemError {
    Io(std::io::Error),
}

impl Display for FileSystemError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "Filesystem error: {error}"),
        }
    }
}

impl Error for FileSystemError {}

pub trait FileSystem: Send + Sync {
    fn path_exists(&self, path: &Path) -> bool;
    fn remove_dir_all(&self, path: &Path) -> Result<(), FileSystemError>;
    fn create_dir_all(&self, path: &Path) -> Result<(), FileSystemError>;
    fn write_string(&self, path: &Path, contents: &str) -> Result<(), FileSystemError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFileSystem;

impl FileSystem for LocalFileSystem {
    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn remove_dir_all(&self, path: &Path) -> Result<(), FileSystemError> {
        std::fs::remove_dir_all(path).map_err(FileSystemError::Io)
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), FileSystemError> {
        std::fs::create_dir_all(path).map_err(FileSystemError::Io)
    }

    fn write_string(&self, path: &Path, contents: &str) -> Result<(), FileSystemError> {
        std::fs::write(path, contents).map_err(FileSystemError::Io)
    }
}
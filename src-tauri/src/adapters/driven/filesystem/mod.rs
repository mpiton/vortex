//! Filesystem adapter — file I/O and `.vortex-meta` persistence.

mod download_dir;
mod file_opener;
mod file_storage;
mod meta_storage;

pub use download_dir::resolve_system_download_dir;
pub use file_opener::SystemFileOpener;
pub use file_storage::FsFileStorage;

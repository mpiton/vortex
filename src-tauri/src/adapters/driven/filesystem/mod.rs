//! Filesystem adapter — file I/O and `.vortex-meta` persistence.

mod file_storage;
mod meta_storage;

pub use file_storage::FsFileStorage;

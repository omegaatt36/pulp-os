// host stand-in for kernel/src/drivers/dir_entry.rs: the reader harness never
// lists directories, so the card shim in storage.rs owns the placeholder types
pub use super::storage::{DirEntry, TITLE_CAP};

pub mod commands;

use std::fmt;

#[derive(Debug)]
pub struct BackupError;

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Backup error")
    }
}

impl std::error::Error for BackupError {}
pub type BackupResult<T> = error_stack::Result<T, BackupError>;

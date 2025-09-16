use std::fmt;

pub mod commands;

#[derive(Debug)]
pub struct MigrateError;
impl fmt::Display for MigrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Migrate error")
    }
}
impl std::error::Error for MigrateError {}

pub type MigrateResult<T> = error_stack::Result<T, MigrateError>;
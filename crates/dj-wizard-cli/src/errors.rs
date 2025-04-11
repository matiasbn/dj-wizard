use std::fmt;

#[derive(Debug)]
pub struct CleanerError;

impl fmt::Display for CleanerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Cleaner error")
    }
}

impl std::error::Error for CleanerError {}

pub type CleanerResult<T> = error_stack::Result<T, CleanerError>;

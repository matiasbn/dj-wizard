pub mod commands;

use ::serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug)]
pub struct QueueError;

impl fmt::Display for QueueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Queue error")
    }
}

impl std::error::Error for QueueError {}

pub type QueueResult<T> = error_stack::Result<T, QueueError>;

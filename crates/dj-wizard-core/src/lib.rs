// crates/dj-wizard-core/src/lib.rs
pub mod error;
pub mod user;
// Add other modules here as you move them
// pub mod soundeo;
// pub mod spotify;
// pub mod log;
// pub mod cleaner;
// pub mod ipfs;

// Re-export main error types
pub use error::{CoreError, Result};

pub mod config;
pub mod engine;
pub mod error;
pub mod formatter;
pub mod model;
pub mod provider;
pub mod recipe;
pub mod rule;

#[cfg(any(test, feature = "test-helpers"))]
pub mod testing;

pub use error::CoreError;

pub type Result<T> = std::result::Result<T, CoreError>;

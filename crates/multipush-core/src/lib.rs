pub mod config;
pub mod engine;
pub mod error;
pub mod formatter;
pub mod model;
pub mod provider;
pub mod recipe;
pub mod rule;

pub use error::CoreError;

pub type Result<T> = std::result::Result<T, CoreError>;

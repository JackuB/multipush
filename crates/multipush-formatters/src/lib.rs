//! Output formatters for multipush.
//!
//! Provides table, JSON, and markdown formatters that implement the
//! `multipush_core::formatter::Formatter` trait.

mod markdown;
mod table;

pub use markdown::MarkdownFormatter;
pub use table::TableFormatter;

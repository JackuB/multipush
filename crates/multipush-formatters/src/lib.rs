//! Output formatters for multipush.
//!
//! Provides table, JSON, and markdown formatters that implement the
//! `multipush_core::formatter::Formatter` trait.

mod markdown;
mod sarif;
mod table;

pub use markdown::MarkdownFormatter;
pub use sarif::SarifFormatter;
pub use table::TableFormatter;

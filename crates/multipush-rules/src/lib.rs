//! Built-in rule implementations for multipush.
//!
//! Provides `EnsureFile`, `EnsureJsonKey`, `EnsureYamlKey`, and `FileMatches`
//! rules that implement the `multipush_core::rule::Rule` trait.

mod ensure_file;

pub use ensure_file::EnsureFileRule;

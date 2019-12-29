use async_std::path::PathBuf;
use std::default::Default;

/// Represents the target for search.
#[derive(Debug, PartialEq)]
pub(crate) enum SearchTarget {
    /// Indicates search will be directed against piped standard input.
    Stdin,

    /// Indicates search will be directed recursively on the current directory.
    CurrentDir,

    /// Indicates search will be directed against the given files/directories.
    SpecifiedPaths(Vec<PathBuf>),
}

impl Default for SearchTarget {
    fn default() -> Self {
        SearchTarget::CurrentDir
    }
}

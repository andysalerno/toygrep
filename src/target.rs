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

#[derive(Debug)]
pub(crate) enum Target {
    Stdin,
    Path(PathBuf),
}

impl Target {
    pub(crate) fn is_stdin(&self) -> bool {
        if let Target::Stdin = self {
            true
        } else {
            false
        }
    }

    pub(crate) async fn is_dir(&self) -> bool {
        match self {
            Target::Path(path) => path.is_dir().await,
            _ => false,
        }
    }

    pub(crate) async fn is_file(&self) -> bool {
        match self {
            Target::Path(path) => path.is_file().await,
            _ => false,
        }
    }

    pub(crate) fn for_path(path: PathBuf) -> Self {
        Target::Path(path)
    }
}

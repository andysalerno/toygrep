use async_std::io::{self, BufReader, Read, Stdin};
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

pub(crate) trait Target<R>
where
    R: Read,
{
    fn name(&self) -> &str;
    fn reader(&self) -> Option<&R>;
    fn children(&self) -> Vec<Box<Self>>;
}

pub(crate) mod std_in {
    use super::*;

    struct StdInTarget<R>
    where
        R: Read,
    {
        name: String,
        reader: R,
    }

    impl<R> StdInTarget<R> {
        pub(crate) fn new<M>(make_reader: M) -> Self
        where
            R: Read,
            M: FnOnce(Stdin) -> R,
        {
            let reader = make_reader(async_std::io::stdin());

            Self {
                reader,
                name: "stdin".into(),
            }
        }
    }

    impl<R> Target<R> for StdInTarget<R>
    where
        R: Read,
    {
        fn name(&self) -> &str {
            &self.name
        }

        fn reader(&self) -> Option<&R> {
            Some(&self.reader)
        }

        fn children(&self) -> Vec<Box<Self>> {
            // Stdin never has children.
            Vec::new()
        }
    }
}

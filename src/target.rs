use async_std::path::PathBuf;

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

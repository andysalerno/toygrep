use async_std::path::{Path, PathBuf};
use crossbeam_channel::Sender;
use ignore::{Walk, WalkBuilder, WalkState};

pub(crate) fn make_walker(root: &Path) -> impl Iterator<Item = PathBuf> {
    Walk::new(root)
        .filter(|r| r.is_ok())
        .map(|r| r.unwrap().into_path().into())
}

pub(crate) fn spawn_parallel_walker(root: &Path, sender: Sender<PathBuf>) {
    WalkBuilder::new(root).build_parallel().run(|| {
        Box::new(|path| {
            path.map(|p| {
                while let Err(_) = sender.send(p.clone().into_path().into()) {
                    std::thread::yield_now();
                }
            });
            WalkState::Continue
        })
    });
}

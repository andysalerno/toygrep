use async_std::path::{Path, PathBuf};
use crossbeam_channel::Sender;
use ignore::{WalkBuilder, WalkState};

pub(crate) fn spawn_parallel_walker(root: &Path, sender: Sender<PathBuf>) {
    WalkBuilder::new(root).build_parallel().run(|| {
        Box::new(|path| {
            path.map(|p| {
                while let Err(_) = sender.send(p.clone().into_path().into()) {
                    std::thread::yield_now();
                }
            })
            .unwrap();
            WalkState::Continue
        })
    });
}

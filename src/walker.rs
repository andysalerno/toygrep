use async_std::path::PathBuf;
use core::future::Future;

struct Walker<F>
where F: Fn() -> Future<Output = ()> + Clone + Send + 'static
{
    path: PathBuf,
    action: F 
}

impl<F> Walker<F>
where F: Fn() -> Future<Output = ()> + Clone + Send + 'static
 {
    fn new(path: PathBuf, action: Box<F>) -> Self {
        Walker { path, action }
    }

    async fn run(self) {
        async_std::task::spawn(self.action).await;
    }

    async fn walk(self) {
        use async_std::prelude::*;

        let mut dir_entries = async_std::fs::read_dir(self.path).await.unwrap();
        while let Some(entry) = dir_entries.next().await {
            let entry_path = entry.unwrap().path();
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // Pick up here:
    // https://github.com/BurntSushi/ripgrep/blob/master/crates/ignore/src/walk.rs#L1193

    #[test]
    fn try_run_future() {
        async_std::task::block_on(async {
            let walker = Walker::new("test".into(), async {
                println!("\n\n\nyo!!\n\n");
                assert!(false, "yo!!!");
            });

            walker.run().await;
        });
    }

}
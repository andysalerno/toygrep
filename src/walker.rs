use async_std::path::PathBuf;
use core::future::Future;

struct Walker<F>
where F: Future<Output = ()> + Send + 'static
{
    path: PathBuf,
    action: F 
}

impl<F> Walker<F>
where F: Future<Output = ()> + Send + 'static
 {
    fn new(path: PathBuf, action: F) -> Self {
        Walker { path, action }
    }

    async fn run(self) {
        async_std::task::spawn(self.action).await;
    }
}

#[cfg(test)]
mod test {
    use super::*;

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
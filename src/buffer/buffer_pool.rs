use super::async_line_buffer::{AsyncLineBuffer, AsyncLineBufferBuilder};
use async_std::sync::Mutex;

#[derive(Default, Debug)]
pub(crate) struct BufferPool {
    pool: Mutex<Vec<AsyncLineBuffer>>,
}

impl BufferPool {
    /// Get a buffer, either recycling an old one, or
    /// generating a fresh one.
    pub(crate) async fn acquire(&self, size_hint: usize) -> AsyncLineBuffer {
        Self::generate_new(size_hint)
        // self.try_get_existing()
        //     .await
        //     .unwrap_or_else(|| Self::generate_new(size_hint))
    }

    pub(crate) fn new() -> BufferPool {
        // let default_size_hint = 8_000;
        // let pool = Mutex::new(
        //     (0..4)
        //         .map(|_| Self::generate_new(default_size_hint))
        //         .collect(),
        // );

        let pool = Default::default();

        Self { pool }
    }

    pub(crate) async fn return_to_pool(&self, mut buf: AsyncLineBuffer) {
        // buf.refresh();
        // self.pool.lock().await.push(buf);
    }

    pub(crate) async fn pool_size(&self) -> usize {
        10
        // dbg!(self.pool.lock().await.len())
    }

    fn generate_new(size_hint: usize) -> AsyncLineBuffer {
        // let size_hint = dbg!(usize::min(6_000_000, size_hint));

        AsyncLineBufferBuilder::new()
            .with_start_size_bytes(size_hint)
            .build()
    }
}

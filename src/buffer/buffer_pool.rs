use super::async_line_buffer::{AsyncLineBuffer, AsyncLineBufferBuilder};
use async_std::sync::Mutex;

#[derive(Default, Debug)]
pub(crate) struct BufferPool {
    pool: Mutex<Vec<AsyncLineBuffer>>,
}

impl BufferPool {
    /// Get a buffer, either recycling an old one, or
    /// generating a fresh one.
    pub(crate) async fn acquire(&self) -> AsyncLineBuffer {
        self.try_get_existing()
            .await
            .unwrap_or_else(Self::generate_new)
    }

    pub(crate) fn new() -> BufferPool {
        let pool = Mutex::new((0..4).map(|_| Self::generate_new()).collect());

        Self { pool }
    }

    pub(crate) async fn return_to_pool(&self, buf: AsyncLineBuffer) {
        self.pool.lock().await.push(buf);
    }

    pub(crate) async fn pool_size(&self) -> usize {
        self.pool.lock().await.len()
    }

    fn generate_new() -> AsyncLineBuffer {
        AsyncLineBufferBuilder::new().build()
    }

    async fn try_get_existing(&self) -> Option<AsyncLineBuffer> {
        let maybe_buf = self.pool.lock().await.pop();

        maybe_buf.and_then(|mut b| {
            b.refresh();

            Some(b)
        })
    }
}

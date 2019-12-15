use async_std::prelude::*;
use std::collections::VecDeque;

/// Strategy: fill as much as you can,
///             then read as much as you can; repeat.
///             Line doesn't fit? Grow buffer.
///             Question: do we expand during filling? not sure yet
/// An asynchronous line buffer.
/// If this is being used to buffer content
/// from a file, a good strategy would be to
/// initialize this with at least as much pre-allocated space
/// as the file size (for reasonably sized files)
/// so only one read from the file will be necessary.
#[derive(Debug, Default)]
struct AsyncLineBuffer {
    /// The internal buffer.
    buffer: Vec<u8>,

    /// The single byte representing a newline.
    /// Since strings are utf8, we are expecting
    /// this has a unique single-byte value
    /// (`\n` fulfills this property).
    line_break_byte: u8,

    /// The locations within the buffer
    /// (relative to the beginning)
    /// where line breaks are known to exist.
    /// Will always be in increasing order.
    line_break_idxs: VecDeque<usize>,

    /// If we attempt to fill this buffer,
    /// and there is not at least this much free space,
    /// more space will be allocated to guarantee
    /// at least this much free space.
    minimum_read_size: usize,

    /// The index of the first unconsumed byte
    /// in the buffer.
    /// E.x: say the buffer has the word "hello\n" populating
    /// indexes 0-5.
    /// `start` is currently 0, indicating nothing has been consumed yet.
    /// We run "consume_line", which returns a slice starting at `start`
    /// until the first newline (so, 0-5), and then updates `start` to begin
    /// directly after this slice, such that a subsequent call will return
    /// a slice of the next newline.
    start: usize,

    /// The first position in the buffer outside
    /// of our written segment.
    /// E.g., if our written segment has len 0, this is 0.
    end: usize,
}

impl AsyncLineBuffer {
    /// Returns a writable slice for the portion
    /// of the internal buffer that is writable.
    /// Note that this may have length 0. Invoke `ensure_capacity()`
    /// to guarantee space is available here.
    fn writable_buffer_mut(&mut self) -> &mut [u8] {
        &mut self.buffer[self.end..]
    }

    fn writable_buffer(&self) -> &[u8] {
        &self.buffer[self.end..]
    }

    fn written_buffer(&self) -> &[u8] {
        &self.buffer[self.start..self.end]
    }

    /// Resize the internal buffer if necessary
    /// to guarantee there is at least `minimum_read_size`
    /// available for writing to.
    fn ensure_capacity(&mut self) {
        if self.writable_buffer().len() < self.minimum_read_size {
            let diff = self.minimum_read_size - self.writable_buffer().len();
            let new_size = self.buffer.len() + diff;
            self.buffer.resize(new_size, 0u8);
        }
    }

    /// Read asynchronously from the reader until the reader
    /// is exhausted, or the writable portion of this
    /// buffer has become full.
    async fn fill<R>(&mut self, mut reader: R) -> bool
    where
        R: async_std::io::Read + std::marker::Unpin,
    {
        self.ensure_capacity();

        let bytes_count = reader
            .read(self.writable_buffer_mut())
            .await
            .expect("Unable to read from reader.");

        // Keep track of any newlines we inserted
        {
            let mut temp_idxs = VecDeque::new();

            // TODO: bit of a hack -- better way to appease borrow checker?
            // Create in-mem vec or clone instead?
            std::mem::swap(&mut temp_idxs, &mut self.line_break_idxs);

            for (idx, _) in self
                .writable_buffer()
                .iter()
                .enumerate()
                .filter(|&(_, &byte)| byte == self.line_break_byte)
            {
                let absolute_pos = self.start + idx;
                temp_idxs.push_front(absolute_pos);
            }
            std::mem::swap(&mut temp_idxs, &mut self.line_break_idxs);
        }

        self.end += bytes_count;

        bytes_count != 0
    }

    /// Retrieve a slice containing the next line,
    /// or None if there is no line.
    /// Internally, the next line starts at `self.start`,
    /// and after calling this, `self.start` will be advanced
    /// by the length of the returned line.
    fn consume_line(&mut self) -> Option<&[u8]> {
        if let Some(line_break_pos) = self.line_break_idxs.pop_back() {
            let line = &self.buffer[self.start..line_break_pos];
            self.start += line.len();

            Some(line)
        } else {
            None
        }
    }

    /// Clear out the consumed portion of the buffer
    /// by rolling the unconsumed content back to the front.
    fn roll_to_front(&mut self) {
        self.buffer.copy_within(self.start..self.end, 0);

        // todo - must update all line_break_idx also...
        self.end -= self.start;
        self.start = 0;
    }
}

struct AsyncLineBufferReader<R>
where
    R: async_std::io::Read + std::marker::Unpin,
{
    line_buffer: AsyncLineBuffer,
    reader: R,
}

impl<R> AsyncLineBufferReader<R>
where
    R: async_std::io::Read + std::marker::Unpin,
{
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            line_buffer: AsyncLineBuffer::default(),
        }
    }

    pub async fn read_line(&mut self) -> Option<&[u8]> {
        while self.line_buffer.line_break_idxs.is_empty() {
            // There are currently no full lines in the buffer, so fill it up.
            // (It would be more readable to do this in the `else` below,
            // but unfortunately the borrow checker does not like that...)
            let any_bytes_read = self.line_buffer.fill(&mut self.reader).await;
            if !any_bytes_read {
                // Our reader had nothing left, so if we only have a partial line in the buffer,
                // we need to return it, since it will never get completed.
                return Some(self.line_buffer.written_buffer());
            }
        }

        // At this point, the line buffer is populated
        // with at least one full line (which we consume below), or
        // else it has already been completely exhausted.
        self.line_buffer.consume_line()
    }
}

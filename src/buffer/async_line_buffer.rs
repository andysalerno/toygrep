#![allow(clippy::needless_lifetimes)] // needed or else it warns on read_line() (possible clippy bug?)

use async_std::prelude::*;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub(crate) struct LineResult<'a> {
    line_num: usize,
    text: &'a [u8],
}

impl<'a> LineResult<'a> {
    fn new(text: &'a [u8], line_num: usize) -> Self {
        Self { line_num, text }
    }

    pub(crate) fn line_num(&self) -> usize {
        self.line_num
    }

    pub(crate) fn text(&self) -> &[u8] {
        &self.text
    }
}

pub(crate) struct AsyncLineBufferBuilder {
    line_break_byte: u8,
    start_size_bytes: usize,
}

impl AsyncLineBufferBuilder {
    pub(crate) fn new() -> Self {
        Self {
            line_break_byte: b'\n',
            start_size_bytes: 8 * (1 << 10),
        }
    }

    pub(crate) fn empty() -> Self { 
        Self {
            line_break_byte: b'\n',
            start_size_bytes: 0,
        }
    }

    pub(crate) fn with_line_break_byte(mut self, line_break_byte: u8) -> Self {
        self.line_break_byte = line_break_byte;
        self
    }

    pub(crate) fn with_start_size_bytes(mut self, start_size_bytes: usize) -> Self {
        self.start_size_bytes = start_size_bytes;
        self
    }

    pub(crate) fn build(self) -> AsyncLineBuffer {
        AsyncLineBuffer {
            // TODO: experiment with "reserved space" instead of pre-allocating
            buffer: vec![0u8; self.start_size_bytes],
            line_break_byte: self.line_break_byte,
            line_break_idxs: VecDeque::new(),
            start: 0,
            end: 0,
        }
    }
}

/// Strategy: fill as much as you can,
///             then read as much as you can; repeat.
///             If a whole line doesn't fit, grow buffer.
/// An asynchronous line buffer.
/// If this is being used to buffer content
/// from a file, a good strategy would be to
/// initialize this with at least as much pre-allocated space
/// as the file size (for reasonably sized files)
/// so only one read from the file will be necessary.
#[derive(Debug, Clone)]
pub(crate) struct AsyncLineBuffer {
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
        if bytes_count != 0 {
            let mut temp_idxs = VecDeque::new();

            // TODO: bit of a hack -- better way to appease borrow checker?
            std::mem::swap(&mut temp_idxs, &mut self.line_break_idxs);

            for (idx, _) in self.writable_buffer()[..bytes_count]
                .iter()
                .enumerate()
                .filter(|&(_, &byte)| byte == self.line_break_byte)
            {
                let absolute_pos = self.end + idx;
                temp_idxs.push_front(absolute_pos);
            }
            std::mem::swap(&mut temp_idxs, &mut self.line_break_idxs);
        }

        self.end += bytes_count;

        bytes_count != 0
    }

    /// Refreshes this buffer into a clean state
    /// so it can be used once again.
    pub(crate) fn refresh(&mut self) {
        self.start = 0;
        self.end = 0;
        self.line_break_idxs.clear();
    }

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

    /// Guarantee the writable portion of the buffer has nonzero length,
    /// expanding the buffer if necessary.
    fn ensure_capacity(&mut self) {
        if !self.writable_buffer().is_empty() {
            return;
        }

        let cur_factor = usize::max(1, self.buffer.len());
        let grow_to = cur_factor * 2;
        self.buffer.resize(grow_to, 0u8);
    }

    /// Retrieve a slice containing the next line,
    /// or None if there is no line in the buffer currently.
    fn consume_line(&mut self) -> Option<&[u8]> {
        if let Some(line_break_pos) = self.line_break_idxs.pop_back() {
            // inclusive range to include the linebreak itself.
            let line = &self.buffer[self.start..=line_break_pos];

            // Keep track of our new starting position by advancing `self.start`,
            // which is how we internally represent that this slice has been consumed.
            self.start += line.len();

            Some(line)
        } else {
            None
        }
    }

    fn consume_remaining(&mut self) -> Option<&[u8]> {
        if self.start >= self.end {
            return None;
        }

        let remaining = &self.buffer[self.start..self.end];
        self.start = self.end;

        Some(remaining)
    }

    /// Clear out the consumed portion of the buffer
    /// by rolling the unconsumed content back to the front.
    fn roll_to_front(&mut self) {
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
            self.line_break_idxs.clear();
            return;
        }

        if self.start == 0 {
            return;
        }

        self.buffer.copy_within(self.start..self.end, 0);

        let left_shift_len = self.start;
        self.end -= left_shift_len;

        self.line_break_idxs.iter_mut().for_each(|idx| {
            *idx -= left_shift_len;
        });

        self.start = 0;
    }

    fn has_line(&self) -> bool {
        !self.line_break_idxs.is_empty()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AsyncLineBufferReader<R>
where
    R: async_std::io::Read + std::marker::Unpin,
{
    line_buffer: AsyncLineBuffer,
    reader: R,
    lines_read: usize,
    is_line_nums_enabled: bool,
}

impl<R> AsyncLineBufferReader<R>
where
    R: async_std::io::Read + std::marker::Unpin,
{
    pub(crate) fn new(reader: R, line_buffer: AsyncLineBuffer) -> Self {
        Self {
            reader,
            line_buffer,
            lines_read: 0,
            is_line_nums_enabled: true,
        }
    }

    pub(crate) fn line_nums(mut self, enabled: bool) -> Self {
        self.is_line_nums_enabled = enabled;
        self
    }

    pub(crate) fn inner_buf_len(&self) -> usize {
        self.line_buffer.buffer.len()
    }

    /// `None` if there are no lines remaining to read.
    /// `Some(Ok(...))` if a line was read and parsed successfully.
    /// `Some(Err(...))` if a line was read but failed to parse.
    pub(crate) async fn read_line<'a>(&'a mut self) -> Option<LineResult<'a>> {
        self.lines_read += 1;
        let line_num = self.lines_read;

        while !self.line_buffer.has_line() {
            self.line_buffer.roll_to_front();
            // There are currently no full lines in the buffer, so fill it up.
            let any_bytes_read = self.line_buffer.fill(&mut self.reader).await;
            if !any_bytes_read {
                // Our reader had nothing left, so if we only have a partial line in the buffer,
                // we need to return it, since it will never get completed.
                let line = self.line_buffer.consume_remaining();

                return line.map(|l| LineResult::new(l, line_num));
            }
        }

        // At this point, the line buffer is populated
        // with at least one full line (which we consume below), or
        // else it has already been completely exhausted.
        let line = self.line_buffer.consume_line();
        line.map(|l| LineResult::new(l, line_num))
    }

    /// Takes the line buffer from this Reader,
    /// so it may be used again. Consumes self.
    pub(crate) fn take_line_buffer(self) -> AsyncLineBuffer {
        self.line_buffer
    }
}

#[cfg(test)]
#[allow(clippy::string_lit_as_bytes)]
mod test {
    use super::*;
    use async_std::io::BufReader;

    #[test]
    fn buffer_reads_simple_text_no_linebreak() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_start_size_bytes(128)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        let line = async_std::task::block_on(async { reader.read_line().await });

        assert_eq!("This is a simple test.".as_bytes(), line.unwrap().text());
    }

    #[test]
    fn buffer_reads_simple_text_one_linebreak() {
        let bytes_reader =
            BufReader::new("This is a simple test.\nAnd now it has two lines.".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_start_size_bytes(128)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        let line = async_std::task::block_on(async { reader.read_line().await });
        assert_eq!("This is a simple test.\n".as_bytes(), line.unwrap().text());

        let line = async_std::task::block_on(async { reader.read_line().await });
        assert_eq!("And now it has two lines.".as_bytes(), line.unwrap().text());
    }

    #[test]
    fn buffer_reads_simple_text_tiny_buffer() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_start_size_bytes(1)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        let line = async_std::task::block_on(async { reader.read_line().await });
        assert_eq!("This is a simple test.".as_bytes(), line.unwrap().text());
    }

    #[test]
    fn buffer_reads_simple_text_two_lines_tiny_buffer() {
        let bytes_reader =
            BufReader::new("This is a simple test.\nAnd now it has two lines.".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_start_size_bytes(1)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        let line = async_std::task::block_on(async { reader.read_line().await });
        assert_eq!("This is a simple test.\n".as_bytes(), line.unwrap().text());

        let line = async_std::task::block_on(async { reader.read_line().await });
        assert_eq!("And now it has two lines.".as_bytes(), line.unwrap().text());
    }

    #[test]
    fn buffer_gives_none_when_no_more_lines_tiny_buffer() {
        let bytes_reader =
            BufReader::new("This is a simple test.\nAnd now it has two lines.".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_start_size_bytes(1)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            reader.read_line().await;
            reader.read_line().await;

            let line = reader.read_line().await;

            assert!(
                line.is_none(),
                "There were only two lines, and two lines were consumed already, \
                 so this should give None."
            );
        });
    }

    #[test]
    fn buffer_gives_none_when_no_more_lines_big_buffer() {
        let bytes_reader =
            BufReader::new("This is a simple test.\nAnd now it has two lines.".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_start_size_bytes(1024)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            reader.read_line().await;
            reader.read_line().await;

            let line = reader.read_line().await;

            assert!(
                line.is_none(),
                "There were only two lines, and two lines were consumed already, \
                 so this should give None."
            );
        });
    }

    #[test]
    fn buffer_handles_empty_string() {
        let bytes_reader = BufReader::new("".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_start_size_bytes(128)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            let line = reader.read_line().await;

            assert!(
                line.is_none(),
                "The reader was totally empty, so the result should be None."
            );
        });
    }

    #[test]
    fn buffer_handles_single_newline() {
        let bytes_reader = BufReader::new("\n".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_start_size_bytes(128)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            let line = reader.read_line().await;

            assert_eq!("\n".as_bytes(), line.unwrap().text());
        });
    }

    #[test]
    fn buffer_handles_multiple_newline() {
        let bytes_reader = BufReader::new("\n\n\n\n".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_start_size_bytes(128)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            let line = reader.read_line().await;
            assert_eq!("\n".as_bytes(), line.unwrap().text());

            let line = reader.read_line().await;
            assert_eq!("\n".as_bytes(), line.unwrap().text());

            let line = reader.read_line().await;
            assert_eq!("\n".as_bytes(), line.unwrap().text());

            let line = reader.read_line().await;
            assert_eq!("\n".as_bytes(), line.unwrap().text());

            let line = reader.read_line().await;
            assert!(line.is_none());
        });
    }

    #[test]
    fn buffer_handles_single_byte_str() {
        let bytes_reader = BufReader::new("H".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_start_size_bytes(128)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            let line = reader.read_line().await;
            assert_eq!("H".as_bytes(), line.unwrap().text());

            let line = reader.read_line().await;
            assert!(line.is_none());
        });
    }

    #[test]
    fn buffer_macbeth() {
        let macbeth = "
Out, damned spot! out, I say!--One: two: why,
then, 'tis time to do't.--Hell is murky!--Fie, my
lord, fie! a soldier, and afeard? What need we
fear who knows it, when none can call our power to
account?--Yet who would have thought the old man
to have had so much blood in him.
        "
        .trim();
        let bytes_reader = BufReader::new(macbeth.as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_start_size_bytes(64)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            let line = reader.read_line().await.unwrap();
            assert_eq!(
                "Out, damned spot! out, I say!--One: two: why,\n".as_bytes(),
                line.text()
            );

            let line = reader.read_line().await.unwrap();
            assert_eq!(
                "then, 'tis time to do't.--Hell is murky!--Fie, my\n".as_bytes(),
                line.text()
            );

            let line = reader.read_line().await.unwrap();
            assert_eq!(
                "lord, fie! a soldier, and afeard? What need we\n".as_bytes(),
                line.text()
            );

            let line = reader.read_line().await.unwrap();
            assert_eq!(
                "fear who knows it, when none can call our power to\n".as_bytes(),
                line.text()
            );

            let line = reader.read_line().await.unwrap();
            assert_eq!(
                "account?--Yet who would have thought the old man\n".as_bytes(),
                line.text()
            );

            let line = reader.read_line().await.unwrap();
            assert_eq!("to have had so much blood in him.".as_bytes(), line.text());
        });
    }
}

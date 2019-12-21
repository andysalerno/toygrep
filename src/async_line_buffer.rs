#![allow(clippy::needless_lifetimes)] // needed or else it warns on read_line() (possible clippy bug?)

use async_std::prelude::*;
use std::collections::VecDeque;
use std::str;

pub(crate) struct LineResult {
    line_num: usize,
    text: String,
}

impl LineResult {
    fn new(text: String, line_num: usize) -> Self {
        Self { line_num, text }
    }

    pub(crate) fn line_num(&self) -> usize {
        self.line_num
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

pub(crate) struct AsyncLineBufferBuilder {
    line_break_byte: u8,
    min_read_size: usize,
}

impl AsyncLineBufferBuilder {
    pub(crate) fn new() -> Self {
        Self {
            line_break_byte: b'\n',
            min_read_size: 4096,
        }
    }

    pub(crate) fn with_line_break_byte(mut self, line_break_byte: u8) -> Self {
        self.line_break_byte = line_break_byte;
        self
    }

    pub(crate) fn with_minimum_read_size(mut self, min_read_size: usize) -> Self {
        self.min_read_size = min_read_size;
        self
    }

    pub(crate) fn build(self) -> AsyncLineBuffer {
        AsyncLineBuffer {
            // TODO: experiment with "reserved space" instead of pre-allocating
            buffer: vec![0u8; self.min_read_size],
            line_break_byte: self.line_break_byte,
            line_break_idxs: VecDeque::new(),
            min_read_size: self.min_read_size,
            start: 0,
            end: 0,
        }
    }
}

/// Strategy: fill as much as you can,
///             then read as much as you can; repeat.
///             Line doesn't fit? Grow buffer.
/// An asynchronous line buffer.
/// If this is being used to buffer content
/// from a file, a good strategy would be to
/// initialize this with at least as much pre-allocated space
/// as the file size (for reasonably sized files)
/// so only one read from the file will be necessary.
#[derive(Debug, Default)]
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

    /// If we attempt to fill this buffer,
    /// and there is not at least this much free space,
    /// more space will be allocated to guarantee
    /// at least this much free space.
    min_read_size: usize,

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
            // Create in-mem vec or clone instead?
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

    /// Resize the internal buffer if necessary
    /// to guarantee there is at least `min_read_size`
    /// available for writing to.
    fn ensure_capacity(&mut self) {
        if self.start == self.end && self.end != 0 {
            // this is an indication the buffer is closed and no longer active.
            return;
        }

        if self.end == self.buffer.len() {
            let grow_to = self.buffer.len() * 2;
            self.buffer.resize(grow_to, 0u8);
        }
    }

    /// Retrieve a slice containing the next line,
    /// or None if there is no line.
    /// Internally, the next line starts at `self.start`,
    /// and after calling this, `self.start` will be advanced
    /// by the length of the returned line.
    fn consume_line(&mut self) -> Option<&[u8]> {
        if let Some(line_break_pos) = self.line_break_idxs.pop_back() {
            // inclusive range to include the linebreak itself.
            let line = &self.buffer[self.start..=line_break_pos];
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

        let remaining = Some(&self.buffer[self.start..self.end]);
        self.start = self.end;

        remaining
    }

    /// Clear out the consumed portion of the buffer
    /// by rolling the unconsumed content back to the front.
    fn roll_to_front(&mut self) {
        if self.start == 0 {
            // Already at the start.
            return;
        }

        self.buffer.copy_within(self.start..self.end, 0);

        // TODO - must update all line_break_idx also...
        let left_shift_len = self.start;
        self.end -= left_shift_len;

        self.line_break_idxs.iter_mut().for_each(|idx| {
            *idx -= left_shift_len;
        });

        self.start = 0;
    }
}

#[derive(Debug)]
pub(crate) struct AsyncLineBufferReader<R>
where
    R: async_std::io::Read + std::marker::Unpin,
{
    line_buffer: AsyncLineBuffer,
    reader: R,
    lines_read: usize,
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
        }
    }

    pub(crate) async fn read_line<'a>(&'a mut self) -> Option<LineResult> {
        self.lines_read += 1;
        let lines_read = self.lines_read;

        let create_result = move |line: Option<&'a [u8]>| {
            line.map(|l| str::from_utf8(l).expect("Line was not valid utf8."))
                .map(|l| LineResult::new(l.to_owned(), lines_read))
        };

        while self.line_buffer.line_break_idxs.is_empty() {
            self.line_buffer.roll_to_front();
            // There are currently no full lines in the buffer, so fill it up.
            let any_bytes_read = self.line_buffer.fill(&mut self.reader).await;
            if !any_bytes_read {
                // Our reader had nothing left, so if we only have a partial line in the buffer,
                // we need to return it, since it will never get completed.
                let line = self.line_buffer.consume_remaining();

                return create_result(line);
            }
        }

        // At this point, the line buffer is populated
        // with at least one full line (which we consume below), or
        // else it has already been completely exhausted.
        let line = self.line_buffer.consume_line();

        create_result(line)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use async_std::io::BufReader;

    #[test]
    fn buffer_reads_simple_text_no_linebreak() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(128)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        let line = async_std::task::block_on(async { reader.read_line().await });

        assert_eq!(b"This is a simple test.", line.unwrap());
    }

    #[test]
    fn buffer_reads_simple_text_one_linebreak() {
        let bytes_reader =
            BufReader::new("This is a simple test.\nAnd now it has two lines.".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(128)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        let line = async_std::task::block_on(async { reader.read_line().await });
        assert_eq!(b"This is a simple test.\n", line.unwrap());

        let line = async_std::task::block_on(async { reader.read_line().await });
        assert_eq!(b"And now it has two lines.", line.unwrap());
    }

    #[test]
    fn buffer_reads_simple_text_tiny_buffer() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(1)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        let line = async_std::task::block_on(async { reader.read_line().await });
        assert_eq!(b"This is a simple test.", line.unwrap());
    }

    #[test]
    fn buffer_reads_simple_text_two_lines_tiny_buffer() {
        let bytes_reader =
            BufReader::new("This is a simple test.\nAnd now it has two lines.".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(1)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        let line = async_std::task::block_on(async { reader.read_line().await });
        assert_eq!(b"This is a simple test.\n", line.unwrap());

        let line = async_std::task::block_on(async { reader.read_line().await });
        assert_eq!(b"And now it has two lines.", line.unwrap());
    }

    #[test]
    fn buffer_gives_none_when_no_more_lines_tiny_buffer() {
        let bytes_reader =
            BufReader::new("This is a simple test.\nAnd now it has two lines.".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(1)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            reader.read_line().await;
            reader.read_line().await;

            let line = reader.read_line().await;

            assert_eq!(
                None, line,
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
            .with_minimum_read_size(1024)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            reader.read_line().await;
            reader.read_line().await;

            let line = reader.read_line().await;

            assert_eq!(
                None, line,
                "There were only two lines, and two lines were consumed already, \
                 so this should give None."
            );
        });
    }

    #[test]
    fn buffer_handles_empty_string() {
        let bytes_reader = BufReader::new("".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(128)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            let line = reader.read_line().await;

            assert_eq!(
                None, line,
                "The reader was totally empty, so the result should be None."
            );
        });
    }

    #[test]
    fn buffer_handles_single_newline() {
        let bytes_reader = BufReader::new("\n".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(128)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            let line = reader.read_line().await;

            assert_eq!(Some("\n".as_bytes()), line);
        });
    }

    #[test]
    fn buffer_handles_multiple_newline() {
        let bytes_reader = BufReader::new("\n\n\n\n".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(128)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            let line = reader.read_line().await;
            assert_eq!(Some("\n".as_bytes()), line);

            let line = reader.read_line().await;
            assert_eq!(Some("\n".as_bytes()), line);

            let line = reader.read_line().await;
            assert_eq!(Some("\n".as_bytes()), line);

            let line = reader.read_line().await;
            assert_eq!(Some("\n".as_bytes()), line);

            let line = reader.read_line().await;
            assert_eq!(None, line);
        });
    }

    #[test]
    fn buffer_handles_single_byte_str() {
        let bytes_reader = BufReader::new("H".as_bytes());

        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(128)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            let line = reader.read_line().await;
            assert_eq!(Some("H".as_bytes()), line);

            let line = reader.read_line().await;
            assert_eq!(None, line);
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
            .with_minimum_read_size(64)
            .build();
        let mut reader = AsyncLineBufferReader::new(bytes_reader, line_buf);

        async_std::task::block_on(async {
            let line = reader.read_line().await.unwrap();
            assert_eq!(
                "Out, damned spot! out, I say!--One: two: why,\n".as_bytes(),
                line
            );

            let line = reader.read_line().await.unwrap();
            assert_eq!(
                "then, 'tis time to do't.--Hell is murky!--Fie, my\n".as_bytes(),
                line
            );

            let line = reader.read_line().await.unwrap();
            assert_eq!(
                "lord, fie! a soldier, and afeard? What need we\n".as_bytes(),
                line
            );

            let line = reader.read_line().await.unwrap();
            assert_eq!(
                "fear who knows it, when none can call our power to\n".as_bytes(),
                line
            );

            let line = reader.read_line().await.unwrap();
            assert_eq!(
                "account?--Yet who would have thought the old man\n".as_bytes(),
                line
            );

            let line = reader.read_line().await.unwrap();
            assert_eq!("to have had so much blood in him.".as_bytes(), line);
        });
    }
}

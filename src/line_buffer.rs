use async_std::io::prelude::*;
use async_std::io::{BufReader, Read};

pub enum ReadLineResult {
    ContinueReading(Vec<u8>),
    EndOfFile(Vec<u8>),
}

pub(crate) struct LineBufferBuilder<R: Read> {
    reader: R,
    min_capacity: usize,
    newline_byte: u8,
}

impl<R: Read + Unpin> LineBufferBuilder<R> {
    fn new(reader: R) -> Self {
        LineBufferBuilder {
            reader,
            min_capacity: 1024,
            newline_byte: b'\n',
        }
    }

    fn with_min_capacity(mut self, min_capacity: usize) -> Self {
        self.min_capacity = min_capacity;
        self
    }

    fn with_newline_byte(mut self, newline_byte: u8) -> Self {
        self.newline_byte = newline_byte;
        self
    }

    fn build(self) -> LineBuffer<R> {
        LineBuffer {
            buffer: vec![0u8; self.min_capacity],
            reader: self.reader,
            min_capacity: self.min_capacity,
            newline_byte: self.newline_byte,

            previous_write_pos_len: None,
        }
    }
}

pub(crate) struct LineBuffer<R: Read> {
    /// Every time the owner wants a writable slice into this buffer,
    /// if there isn't min_capacity room for writing, the internal buffer
    /// will be extended to min_capacity.
    min_capacity: usize,

    /// A tuple of (pos, len) of the previous write,
    /// where "pos" is the index of the first position of the write,
    /// and "len" is the count of bytes written.
    /// None if there has not yet been a write.
    previous_write_pos_len: Option<(usize, usize)>,

    /// The byte that indicates a newline.
    /// Necesssary because this buffer is line-aware.
    /// NOTE: the current expectation is this byte is an ASCII character,
    /// and not part of a multi-byte utf-8 character.
    newline_byte: u8,

    /// The internal buffer. It begins with capacity min_capacity,
    /// and grows as needed with each insertion.
    /// This internal buffer will never shrink in size.
    buffer: Vec<u8>,

    reader: R,
}

impl<R: Read + Unpin> LineBuffer<R> {
    /// The index position where the next write will begin.
    fn next_write_pos(&self) -> usize {
        match self.previous_write_pos_len {
            None => 0,
            Some((pos, len)) => pos + len,
        }
    }

    /// Returns the length remaining in the buffer for writing.
    fn next_writable_len(&self) -> usize {
        let max_buf_len = self.buffer.len();
        let next_write_pos = self.next_write_pos();

        if next_write_pos > max_buf_len {
            0
        } else {
            max_buf_len - next_write_pos
        }
    }

    /// Returns a mutable slice of the next portion of the buffer
    /// that is available for writing into.
    /// This is guaranteed to always be at least as large
    /// as the min_capacity value.
    fn next_writable_slice(&mut self) -> &mut [u8] {
        self.update_buffer_capacity();

        let next_write_pos = self.next_write_pos();

        &mut self.buffer[next_write_pos..]
    }

    /// The length of content currently in the buffer.
    /// E.g., if the buffer was created with capacity for 1024,
    /// but it has never been written into, this will be 0.
    /// After a write of 40 bytes, this will be 40.
    /// After consuming 20 bytes, this will be 20.
    fn content_len(&self) -> usize {
        self.next_write_pos()
    }

    fn update_buffer_capacity(&mut self) {
        let writable_len = self.next_writable_len();

        if writable_len < self.min_capacity {
            let diff = self.min_capacity - writable_len;
            let cur_len = self.buffer.len();
            self.buffer.resize(cur_len + diff, 0u8);
        }
    }

    /// If the most recent write contains a line terminator,
    /// returns the location of the line terminator,
    /// or else None.
    fn previous_write_line_end_pos(&self) -> Option<usize> {
        let prev_write_slice = self.last_written_slice();
        let previous_write_start_pos = self
            .previous_write_pos_len
            .expect("Can't execute this until a write has ocurred.")
            .0;

        prev_write_slice
            .iter()
            .position(|&c| c == self.newline_byte)
            .map(|pos| pos + previous_write_start_pos)
    }

    fn try_drain_line(&mut self) -> Option<Vec<u8>> {
        if let Some(line_break_pos) = self.previous_write_line_end_pos() {
            // + 1 to include the newline itself
            let mut drained_line = self.drain_buf_until(line_break_pos + 1);

            // Pop off the newline, since we don't want it in the result
            drained_line.pop();

            Some(drained_line)
        } else {
            None
        }
    }

    // Drain the buffer up to (but not including) the given position.
    fn drain_buf_until(&mut self, pos: usize) -> Vec<u8> {
        // TODO: more performant to split the vector here?
        // Drain the line, including the newline at the end, and pop it off.
        let drained_line = self.buffer.drain(..pos).collect::<Vec<_>>();

        if let Some((prev_pos, prev_len)) = self.previous_write_pos_len.as_mut() {
            let diff = if pos > *prev_pos { pos - *prev_pos } else { 0 };

            *prev_len -= diff;
            *prev_pos = 0;
        }

        drained_line
    }

    /// Returns a slice containing the content
    /// of the most recent write.
    fn last_written_slice(&self) -> &[u8] {
        let (pos, len) = self
            .previous_write_pos_len
            .expect("Attempted to retrieve a slice before any write has ocurred.");

        &self.buffer[pos..pos + len]
    }

    /// Performs a single read into the buffer.
    /// Returns true if the reader is still not fully consumed.
    async fn perform_single_read(&mut self) -> bool {
        self.update_buffer_capacity();
        let write_pos = self.next_write_pos();
        let writable_slice = &mut self.buffer[write_pos..];

        let written_bytes_count = self
            .reader
            .read(writable_slice)
            .await
            .expect("Failed to read bytes from inner reader.");

        if written_bytes_count > 0 {
            self.previous_write_pos_len = Some((write_pos, written_bytes_count));
        }

        // If we filled the entire buffer, the reader probably still has content.
        written_bytes_count == writable_slice.len()
    }

    pub async fn read_next_line(&mut self) -> ReadLineResult {
        loop {
            let has_more = self.perform_single_read().await;

            if let Some(line) = self.try_drain_line() {
                return ReadLineResult::ContinueReading(line);
            }

            if !has_more {
                // Nothing left to read, so give back the full content of the buffer
                let last_write_pos_len = self.previous_write_pos_len.unwrap_or((0, 0));
                let end_pos = last_write_pos_len.0 + last_write_pos_len.1;

                // TODO: also split buffer here, instead of drain, for perf?
                let drained_content = self.drain_buf_until(end_pos);
                return ReadLineResult::EndOfFile(drained_content);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    impl ReadLineResult {
        fn expect_continue(self) -> Vec<u8> {
            match self {
                ReadLineResult::EndOfFile(_) => panic!("Expected ContinueReading"),
                ReadLineResult::ContinueReading(v) => v,
            }
        }

        fn expect_eof(self) -> Vec<u8> {
            match self {
                ReadLineResult::ContinueReading(_) => panic!("Expected EndOfFile"),
                ReadLineResult::EndOfFile(v) => v,
            }
        }
    }

    #[test]
    fn buffer_does_not_grow_when_has_capacity() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(1024)
            .build();

        async_std::task::block_on(async {
            line_buf.perform_single_read().await;
        });

        assert_eq!(
            1024,
            line_buf.buffer.len(),
            "Since the min capacity was larger than the amount to be read,
            the internal buffer should not have changed size."
        );
    }

    #[test]
    fn next_write_pos_is_correct() {
        let bytes = vec![0u8; 60];

        let mut small_buf = LineBufferBuilder::new(bytes.as_slice())
            .with_min_capacity(8)
            .build();

        let mut big_buff = LineBufferBuilder::new(bytes.as_slice())
            .with_min_capacity(1024)
            .build();

        async_std::task::block_on(async {
            small_buf.perform_single_read().await;

            assert_eq!(8, small_buf.next_write_pos());

            big_buff.perform_single_read().await;

            assert_eq!(60, big_buff.next_write_pos());
        });
    }

    #[test]
    fn buffer_grows_when_insignificant_capacity() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(8)
            .build();

        async_std::task::block_on(async {
            line_buf.perform_single_read().await;

            // Perform another read, which will require growing the buffer.
            line_buf.perform_single_read().await;
        });

        assert_eq!(
            16,
            line_buf.buffer.len(),
            "The buffer must have grown to accomodate the next read."
        );
    }

    #[test]
    fn perform_single_read_gives_true_if_more_content() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(8)
            .build();

        async_std::task::block_on(async {
            let reader_has_more = line_buf.perform_single_read().await;

            assert!(
                reader_has_more,
                "There is still more to read from the reader."
            );
        });
    }

    #[test]
    fn perform_single_read_gives_false_if_no_more_content() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(8)
            .build();

        async_std::task::block_on(async {
            line_buf.perform_single_read().await;
            line_buf.perform_single_read().await;

            // After a third read, the entire reader should have been consumed.
            let reader_has_more = line_buf.perform_single_read().await;

            assert!(
                !reader_has_more,
                "There should not have been more content in the reader."
            );
        });
    }

    #[test]
    fn last_written_slice_is_correct() {
        let bytes_reader =
            BufReader::new("This is a simple test. With extra characters.".as_bytes());

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(8)
            .build();

        async_std::task::block_on(async {
            // We'll read twice.
            line_buf.perform_single_read().await;
            line_buf.perform_single_read().await;

            let slice = line_buf.last_written_slice();

            assert_eq!(
                "a simple".as_bytes(),
                slice,
                "Expected the contents of the last write."
            );
        });
    }

    // #[test]
    // fn buffer_completes_after_consuming_entire_reader() {
    //     let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

    //     let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //         .with_min_capacity(8)
    //         .build();

    //     async_std::task::block_on(async {
    //         line_buf.perform_single_read().await;

    //         // Perform another read, which will require growing the buffer.
    //         line_buf.perform_single_read().await;

    //         // One more read of this size should finish the entirety of the given reader.
    //         line_buf.perform_single_read().await;
    //     });

    //     let end_pos = line_buf.last_write_end_pos();
    //     let buffer_content = &line_buf.buffer[..end_pos];

    //     assert_eq!(
    //         buffer_content,
    //         "This is a simple test.".as_bytes(),
    //         "The content of the buffer should now be the exact value of the input bytes."
    //     );
    // }

    #[test]
    fn read_next_line_consumes_remaining_reader() {
        let bytes = "This is a simple test.".as_bytes();
        let bytes_reader = BufReader::new(bytes);

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(8)
            .build();

        async_std::task::block_on(async {
            let line_read = line_buf.read_next_line().await;

            if let ReadLineResult::EndOfFile(line) = line_read {
                assert_eq!(
                    bytes,
                    line.as_slice(),
                    "Expected the read content to match the input content."
                );
            } else {
                assert!(false, "Expected EndOfFile for the read line result.");
            }
        });
    }

    #[test]
    fn read_next_line_reads_line_when_below_capacity() {
        let bytes_reader = BufReader::new(
            "This is a simple test.\nAnd this is another line in the test.".as_bytes(),
        );

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(1024)
            .build();

        async_std::task::block_on(async {
            let line_read = line_buf.read_next_line().await;

            assert_eq!(
                line_read.expect_continue().as_slice(),
                "This is a simple test.".as_bytes(),
                "Expected the read content to match the input content."
            );
        });
    }

    #[test]
    fn try_drain_resulting_line_gives_correct_result_when_enough_capacity() {
        let bytes_reader = BufReader::new(
            "This is a simple test.\nAnd this is another line in the test.".as_bytes(),
        );

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(1024)
            .build();

        async_std::task::block_on(async {
            line_buf.perform_single_read().await;

            let drained = line_buf
                .try_drain_line()
                .expect("Must have the given line.");

            assert_eq!("This is a simple test.".as_bytes(), drained.as_slice());
        });
    }

    #[test]
    fn try_drain_resulting_line_gives_correct_result_when_not_enough_capacity() {
        let bytes_reader = BufReader::new(
            "This is a simple test.\nAnd this is another line in the test.".as_bytes(),
        );

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(8)
            .build();

        async_std::task::block_on(async {
            line_buf.perform_single_read().await;

            let drained = line_buf.try_drain_line();

            assert!(
                drained.is_none(),
                "One read was not enough to provide a full line with this capacity."
            );
        });
    }

    #[test]
    fn try_drain_resulting_line_gives_correct_result_after_multiple_reads() {
        let bytes_reader = BufReader::new(
            "This is a simple test.\nAnd this is another line in the test.".as_bytes(),
        );

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(8)
            .build();

        async_std::task::block_on(async {
            // Three reads required to complete the first line.
            line_buf.perform_single_read().await;
            line_buf.perform_single_read().await;
            line_buf.perform_single_read().await;

            let drained = line_buf
                .try_drain_line()
                .expect("Must have the given line.");

            assert_eq!("This is a simple test.".as_bytes(), drained.as_slice());
        });
    }

    #[test]
    fn buffer_has_expected_state_after_draining_line() {
        let bytes_reader = BufReader::new(
            "This is a simple test.\nAnd this is another line in the test.".as_bytes(),
        );

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(128)
            .build();

        async_std::task::block_on(async {
            line_buf.perform_single_read().await;

            let drained = line_buf
                .try_drain_line()
                .expect("Must have the given line.");

            assert_eq!(37, line_buf.content_len());
        });
    }

    #[test]
    fn read_next_line_reads_two_lines() {
        let bytes_reader = BufReader::new(
            "This is a simple test.\nAnd this is another line in the test.".as_bytes(),
        );

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(128)
            .build();

        async_std::task::block_on(async {
            let line_read = line_buf.read_next_line().await;

            assert_eq!(
                "This is a simple test.".as_bytes(),
                line_read.expect_continue().as_slice(),
                "Expected the read content to match the input content."
            );

            let line_read = line_buf.read_next_line().await;

            assert_eq!(
                "And this is another line in the test.".as_bytes(),
                line_read.expect_eof().as_slice(),
                "Expected the read content to match the input content."
            );
        });
    }

    #[test]
    fn read_next_line_reads_three_lines_with_big_buffer() {
        let bytes_reader = BufReader::new(
            "This is a simple test.\nAnd this is another line in the test.\nAnd this is one last, third line.".as_bytes(),
        );

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(1024)
            .build();

        async_std::task::block_on(async {
            let line_read = line_buf.read_next_line().await;

            assert_eq!(
                "This is a simple test.".as_bytes(),
                line_read.expect_continue().as_slice(),
                "Expected the read content to match the input content."
            );

            let line_read = line_buf.read_next_line().await;

            assert_eq!(
                "And this is another line in the test.".as_bytes(),
                line_read.expect_continue().as_slice(),
                "Expected the read content to match the input content."
            );

            let line_read = line_buf.read_next_line().await;

            assert_eq!(
                "And this is one last, third line.".as_bytes(),
                line_read.expect_eof().as_slice(),
                "Expected the read content to match the input content."
            );
        });
    }

    #[test]
    fn read_next_line_reads_three_lines_with_little_buffer() {
        let bytes_reader = BufReader::new(
            "This is a simple test.\nAnd this is another line in the test.\nAnd this is one last, third line.".as_bytes(),
        );

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(8)
            .build();

        async_std::task::block_on(async {
            let line_read = line_buf.read_next_line().await;

            assert_eq!(
                "This is a simple test.".as_bytes(),
                line_read.expect_continue().as_slice(),
                "Expected the read content to match the input content."
            );

            let line_read = line_buf.read_next_line().await;

            assert_eq!(
                "And this is another line in the test.".as_bytes(),
                line_read.expect_continue().as_slice(),
                "Expected the read content to match the input content."
            );

            let line_read = line_buf.read_next_line().await;

            assert_eq!(
                "And this is one last, third line.".as_bytes(),
                line_read.expect_eof().as_slice(),
                "Expected the read content to match the input content."
            );
        });
    }

    #[test]
    fn read_next_line_gives_zero_byte_vec_when_no_more_data() {
        let bytes_reader = BufReader::new(
            "This is a simple test.\nAnd this is another line in the test.\nAnd this is one last, third line.".as_bytes(),
        );

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(8)
            .build();

        async_std::task::block_on(async {
            let _ = line_buf.read_next_line().await;
            let _ = line_buf.read_next_line().await;

            let line = line_buf.read_next_line().await;

            line.expect_eof();
        });
    }
}

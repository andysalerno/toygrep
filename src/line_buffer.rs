use async_std::io::prelude::*;
use async_std::io::{BufReader, Read};

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

            next_write_pos: 0,
            previous_write_len: 0,
            previous_write_pos: 0,
        }
    }
}

pub(crate) struct LineBuffer<R: Read> {
    /// Every time the owner wants a writable slice into this buffer,
    /// if there isn't min_capacity room for writing, the internal buffer
    /// will be extended to min_capacity.
    min_capacity: usize,

    /// The index location in the internal buffer where the next
    /// write will take place.
    /// I.e., if a writable slice into the buffer is requested,
    /// the slice will begin at this position in the internal buffer.
    next_write_pos: usize,

    /// The index location in the internal buffer where the previous write
    /// ocurred.
    previous_write_pos: usize,

    /// The length of the previous write in the internal buffer.
    previous_write_len: usize,

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
    fn last_write_end_pos(&self) -> usize {
        self.previous_write_pos + self.previous_write_len
    }

    /// Returns the length remaining in the buffer for writing.
    fn next_writable_len(&self) -> usize {
        let max_buf_len = self.buffer.len();
        assert!(self.next_write_pos <= max_buf_len);

        max_buf_len - self.next_write_pos
    }

    /// Returns a mutable slice of the next portion of the buffer
    /// that is available for writing into.
    /// This is guaranteed to always be at least as large
    /// as the min_capacity value.
    fn next_writable_slice(&mut self) -> &mut [u8] {
        self.update_buffer_capacity();

        let next_write_pos = dbg!(self.next_write_pos);

        &mut self.buffer[next_write_pos..]
    }

    fn update_buffer_capacity(&mut self) {
        let writable_len = dbg!(self.next_writable_len());

        if writable_len < dbg!(self.min_capacity) {
            let diff = dbg!(self.min_capacity - writable_len);
            let cur_len = self.buffer.len();
            self.buffer.resize(cur_len + diff, 0u8);
        }
    }

    /// If the most recent call to extend() concluded the line,
    /// returns the location of the line terminator,
    /// or else None.
    fn previous_write_line_end_pos(&self) -> Option<usize> {
        let prev_write_slice = self.previous_write_slice();

        prev_write_slice
            .iter()
            .position(|&c| c == self.newline_byte)
            .map(|pos| pos + self.previous_write_pos)
    }

    fn try_drain_resulting_line(&mut self) -> Option<Vec<u8>> {
        if let Some(pos) = self.previous_write_line_end_pos() {
            let drained_line = self.buffer.drain(..pos).collect::<Vec<_>>();

            Some(drained_line)
        } else {
            None
        }
    }

    /// Returns a slice containing the content
    /// of the most recent write.
    fn previous_write_slice(&self) -> &[u8] {
        let pwp = self.previous_write_pos;
        let ep = pwp + self.previous_write_len;

        &self.buffer[pwp..ep]
    }

    /// Performs a single read into the buffer.
    /// Returns true if the reader is still not fully consumed.
    async fn perform_single_read(&mut self) -> bool {
        self.update_buffer_capacity();
        let write_pos = self.next_write_pos;
        let writable_slice = &mut self.buffer[write_pos..];

        let written_bytes_count = self
            .reader
            .read(writable_slice)
            .await
            .expect("Failed to read bytes from inner reader.");

        self.previous_write_pos = self.next_write_pos;
        self.previous_write_len = written_bytes_count;
        self.next_write_pos = self.previous_write_pos + written_bytes_count;

        // If we filled the entire buffer, the reader probably still has content.
        written_bytes_count == writable_slice.len()
    }

    pub async fn read_next_line(&mut self) -> Vec<u8> {
        loop {
            self.perform_single_read().await;

            if let Some(line) = self.try_drain_resulting_line() {
                return line;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

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
    fn buffer_completes_after_consuming_entire_reader() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let mut line_buf = LineBufferBuilder::new(bytes_reader)
            .with_min_capacity(8)
            .build();

        async_std::task::block_on(async {
            line_buf.perform_single_read().await;

            // Perform another read, which will require growing the buffer.
            line_buf.perform_single_read().await;

            // One more read of this size should finish the entirety of the given reader.
            line_buf.perform_single_read().await;
        });

        let end_pos = line_buf.last_write_end_pos();
        let buffer_content = &line_buf.buffer[..end_pos];

        assert_eq!(
            buffer_content,
            "This is a simple test.".as_bytes(),
            "The content of the buffer should now be the exact value of the input bytes."
        );
    }

    // #[test]
    // fn buffer_next_writable_len_decreases_after_write() {
    //     let test_bytes = "This is a simple test.";

    //     let mut line_buf = LineBuffer::with_min_capacity(1024);

    //     assert_eq!(1024, line_buf.next_writable_len());

    //     write!(line_buf.next_writable_slice(), "{}", test_bytes)
    //         .expect("Failed writing into buffer.");

    //     line_buf.record_write_len(dbg!(test_bytes.bytes().len()));

    //     assert_eq!(1002, line_buf.next_writable_len(),
    //         "The remaining writable length should have been decreased by the size of the written data.");
    // }

    // #[test]
    // fn buffer_grows_when_needs_capacity() {
    //     let test_bytes = "Hello, everyone!!!";

    //     let mut line_buf = LineBuffer::with_min_capacity(16);

    //     let mut writable = line_buf.next_writable_slice();
    //     let writable_len = writable.len();

    //     let writable_bytes = &test_bytes[..writable_len];

    //     write!(writable, "{}", writable_bytes).expect("Failed writing into buffer.");
    //     line_buf.record_write_len(writable_len);

    //     let mut another_writable = line_buf.next_writable_slice();
    //     assert_eq!(16, another_writable.len());

    //     write!(another_writable, "More writing!").expect("Failed writing into buffer.");
    // }

    // #[test]
    // fn previous_write_line_end_pos_is_none_when_no_newline() {
    //     let test_bytes = "Hello, everyone!!!";

    //     let mut line_buf = LineBuffer::with_min_capacity(16);

    //     write!(line_buf.)
    // }
}

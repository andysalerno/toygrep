pub(crate) struct LineBuffer {
    min_capacity: usize,
    next_write_pos: usize,
    previous_write_pos: usize,
    previous_write_len: usize,

    /// The byte that indicates a newline.
    /// Since this buffer is line-aware, it must
    /// have the knowledge of what delimits a line.
    newline_byte: u8,

    buffer: Vec<u8>,
}

impl Default for LineBuffer {
    fn default() -> Self {
        const DEFAULT_MIN_CAPACITY: usize = 1024;
        LineBuffer::with_min_capacity(DEFAULT_MIN_CAPACITY)
    }
}

impl LineBuffer {
    fn with_min_capacity(min_capacity: usize) -> Self {
        LineBuffer {
            min_capacity,
            buffer: vec![0u8; min_capacity],

            newline_byte: b'\n',

            next_write_pos: 0,
            previous_write_pos: 0,
            previous_write_len: 0,
        }
    }

    /// Returns the full internal buffer as a mutable slice.
    fn internal_full_slice(&mut self) -> &mut [u8] {
        self.buffer.as_mut_slice()
    }

    /// Returns the current max size of the internal buffer.
    fn internal_max_len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns the length remaining in the buffer for writing.
    fn next_writable_len(&self) -> usize {
        let max_buf_len = self.internal_max_len();
        assert!(self.next_write_pos <= max_buf_len);

        max_buf_len - self.next_write_pos
    }

    /// Returns a mutable slice of the next portion of the buffer
    /// that is available for writing into.
    /// This is guaranteed to always be at least as large
    /// as the min_capacity value.
    fn next_writable_slice(&mut self) -> &mut [u8] {
        let writable_len = dbg!(self.next_writable_len());

        if writable_len < dbg!(self.min_capacity) {
            let diff = dbg!(self.min_capacity - writable_len);
            let cur_len = self.buffer.len();
            self.buffer.resize(cur_len + diff, 0u8);
        }

        let next_write_pos = dbg!(self.next_write_pos);
        dbg!(self.internal_max_len());

        &mut self.internal_full_slice()[next_write_pos..]
    }

    /// After a write into the buffer, the writer must call this
    /// to update internal state with the length of bytes written.
    /// TODO: possibly, this method can return a different type
    /// that does all the interesting work, so therefore
    /// callers MUST invoke this if they want to actually use the reuslt.
    fn record_write_len(&mut self, bytes_written: usize) {
        self.previous_write_pos = self.next_write_pos;
        self.previous_write_len = bytes_written;
        self.next_write_pos = self.previous_write_pos + bytes_written;
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

    /// If the most recent call to "extend()"
    /// resulted in a completed line, this returns
    /// the slice for that line, and updates the internal
    /// state of the buffer to prepare it for the next line.
    fn drain_resulting_line(&mut self) -> Option<Vec<u8>> {
        if let Some(pos) = self.previous_write_line_end_pos() {
            let drained_line = self.buffer.drain(..pos).collect::<Vec<_>>();

            Some(drained_line)
        } else {
            None
        }
    }

    /// Returns a slice containing the content
    /// of the previous call to extend().
    fn previous_write_slice(&self) -> &[u8] {
        let pwp = self.previous_write_pos;
        let ep = pwp + self.previous_write_len;

        &self.buffer[pwp..ep]
    }

    /// Extends the internal buffer to at least
    /// the size of the minimum length, if necessary.
    fn extend_to_min_capacity(&mut self) {
        if self.buffer.len() < self.min_capacity {}
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::io::Write;

    #[test]
    fn buffer_does_not_grow_when_has_capacity() {
        let test_bytes = "This is a simple test.";

        let mut line_buf = LineBuffer::with_min_capacity(1024);

        write!(line_buf.next_writable_slice(), "{}", test_bytes)
            .expect("Failed writing into buffer.");

        assert_eq!(line_buf.internal_max_len(), 1024,
            "The size of the internal buffer should not have grown, since the written data did not exceed the min size.");
    }

    #[test]
    fn buffer_next_writable_len_decreases_after_write() {
        let test_bytes = "This is a simple test.";

        let mut line_buf = LineBuffer::with_min_capacity(1024);

        assert_eq!(1024, line_buf.next_writable_len());

        write!(line_buf.next_writable_slice(), "{}", test_bytes)
            .expect("Failed writing into buffer.");

        line_buf.record_write_len(dbg!(test_bytes.bytes().len()));

        assert_eq!(1002, line_buf.next_writable_len(),
            "The remaining writable length should have been decreased by the size of the written data.");
    }

    #[test]
    fn buffer_grows_when_needs_capacity() {
        let test_bytes = "Hello, everyone!!!";

        let mut line_buf = LineBuffer::with_min_capacity(16);

        let mut writable = line_buf.next_writable_slice();
        let writable_len = dbg!(writable.len());

        let writable_bytes = dbg!(&test_bytes[..writable_len]);

        write!(writable, "{}", writable_bytes).expect("Failed writing into buffer.");
        line_buf.record_write_len(writable_len);

        let mut another_writable = line_buf.next_writable_slice();
        dbg!(another_writable.len());
        assert!(!another_writable.is_empty());
        write!(another_writable, "More writing!").expect("Failed writing into buffer.");
    }
}

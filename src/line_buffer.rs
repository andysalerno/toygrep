use async_std::io::prelude::*;
use async_std::io::Read;
use std::collections::VecDeque;

pub struct AsyncLineBufferBuilder<R: Read> {
    reader: R,
    max_capacity: Option<usize>,
    newline_byte: u8,
    initial_capacity: usize,
}

impl<R: Read + Unpin> AsyncLineBufferBuilder<R> {
    pub fn new(reader: R) -> Self {
        AsyncLineBufferBuilder {
            reader,
            max_capacity: None,
            newline_byte: b'\n',
            initial_capacity: 1024,
        }
    }

    pub fn with_initial_capacity(mut self, initial_capacity: usize) -> Self {
        self.initial_capacity = initial_capacity;
        self
    }

    pub fn with_newline_byte(mut self, newline_byte: u8) -> Self {
        self.newline_byte = newline_byte;
        self
    }

    pub fn build(self) -> AsyncLineBuffer<R> {
        AsyncLineBuffer {
            buffer: vec![0u8; self.initial_capacity],
            line_break_positions: VecDeque::new(),
            reader: self.reader,
            initial_capacity: self.initial_capacity,
            max_capacity: self.max_capacity,
            newline_byte: self.newline_byte,
            end: 0,
        }
    }
}

#[derive(Debug)]
pub struct AsyncLineBuffer<R: Read> {
    /// The maximum length the internal buffer can reach
    /// after expanding to fit longer lines.
    /// 'None' indicates unlimited possible growth (constrained
    /// by the reality if memory, of course).
    max_capacity: Option<usize>,

    /// Represents a queue of positions within the buffer
    /// where line breaks reside.
    /// I.e., if the buffer contains "two and a half" lines
    /// (two whole lines and one partial line), this will hold the
    /// positions of the two newline positions splitting the lines.
    line_break_positions: VecDeque<usize>,

    /// The starting capacity of the buffer.
    initial_capacity: usize,

    /// The first index in the buffer that is outside the written portion.
    end: usize,

    /// The byte that indicates a newline.
    /// Necesssary because this buffer is line-aware.
    /// NOTE: the current expectation is this byte is an ASCII character,
    /// and not part of a multi-byte utf-8 character.
    newline_byte: u8,

    /// The internal buffer. It begins with capacity min_capacity,
    /// and grows as needed with each insertion.
    /// This internal buffer will never shrink in size.
    buffer: Vec<u8>,

    /// The reader, and the source for this buffer.
    reader: R,
}

impl<R: Read + Unpin> AsyncLineBuffer<R> {
    fn writable_slice(&mut self) -> &mut [u8] {
        &mut self.buffer[self.end..]
    }

    /// Make sure the writable portion of
    /// the buffer is non-empty by growing if necessary.
    fn ensure_capacity(&mut self) {
        const MIN_CAPACITY: usize = 8000;
        if self.writable_slice().len() >= MIN_CAPACITY {
            return;
        }

        let doubled_space = usize::max(1, self.buffer.len()) * 2;
        let resize_to = self
            .max_capacity
            .and_then(|m| Some(usize::min(doubled_space, m)))
            .unwrap_or(doubled_space);

        let resize_to = usize::max(MIN_CAPACITY, resize_to);

        self.buffer.resize(resize_to, 0u8);
    }

    async fn read_to_buffer(&mut self) -> usize {
        self.ensure_capacity();

        let free_buffer = &mut self.buffer[self.end..];
        let bytes_written = self
            .reader
            .read(free_buffer)
            .await
            .expect("Failed reading from buffer.");

        let written_slice = &free_buffer[..bytes_written];

        // TODO: experiment with an iterator here
        for i in 0..written_slice.len() {
            if written_slice[i] == self.newline_byte {
                let global_pos = self.end + i;
                self.line_break_positions.push_back(global_pos);
            }
        }

        self.end += bytes_written;

        bytes_written
    }

    fn drain_to_pos(&mut self, pos: usize) -> Vec<u8> {
        // + 1 to include the line break itself
        let len_pre = self.buffer.len();
        let drained_line = self.buffer.drain(..pos).collect::<Vec<_>>();
        let len_post = self.buffer.len();

        let diff = len_pre - len_post;
        self.end -= diff;
        self.line_break_positions.iter_mut().for_each(|p| {
            *p -= diff;
        });

        drained_line
    }

    pub async fn read_next_line(&mut self) -> Option<Vec<u8>> {
        loop {
            if let Some(break_pos) = self.line_break_positions.pop_front() {
                // We already have a full line in our buffer,
                // no need to grab anything from our reader.
                // +1 to include the linebreak itself
                let mut drained_line = self.drain_to_pos(break_pos + 1);

                // Pop off the line break.
                // drained_line.pop();

                return Some(drained_line);
            }

            let bytes_written = self.read_to_buffer().await;

            if bytes_written == 0 {
                if self.end != 0 {
                    // Our reader has nothing left to give us,
                    // so give *our* reader everything we have left.
                    return Some(self.drain_to_pos(self.end));
                } else {
                    return None;
                }
            }
        }
    }

    // #[cfg(test)]
    fn as_string(&self) -> String {
        String::from_utf8(self.buffer.clone()).expect("Could not interpret buffer as a string.")
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use async_std::io::BufReader;

    #[test]
    fn buffer_does_not_grow_when_has_capacity() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let mut line_buf = AsyncLineBufferBuilder::new(bytes_reader)
            .with_initial_capacity(128)
            .build();

        async_std::task::block_on(async {
            line_buf.read_to_buffer().await;
        });

        assert_eq!(
            128,
            line_buf.buffer.len(),
            "Since the min capacity was larger than the amount to be read,
            the internal buffer should not have changed size."
        );
    }

    #[test]
    fn buffer_grows_when_insignificant_capacity() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let mut line_buf = AsyncLineBufferBuilder::new(bytes_reader)
            .with_initial_capacity(8)
            .build();

        async_std::task::block_on(async {
            line_buf.read_to_buffer().await;
            line_buf.read_to_buffer().await;
            line_buf.read_to_buffer().await;
        });

        assert_eq!(
            32,
            line_buf.buffer.len(),
            "The buffer should have grown to accomodate each read."
        );
    }

    #[test]
    fn buffer_grows_when_insignificant_capacity_2() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let mut line_buf = AsyncLineBufferBuilder::new(bytes_reader)
            .with_initial_capacity(8)
            .build();

        async_std::task::block_on(async {
            line_buf.read_to_buffer().await;
            line_buf.read_to_buffer().await;
            line_buf.read_to_buffer().await;
            line_buf.read_to_buffer().await;
        });

        assert_eq!(
            32,
            line_buf.buffer.len(),
            "The buffer should not grow more than it needs to grow to hold the content."
        );
    }

    #[test]
    fn buffer_grows_when_insignificant_capacity_3() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let mut line_buf = AsyncLineBufferBuilder::new(bytes_reader)
            .with_initial_capacity(8)
            .build();

        async_std::task::block_on(async {
            let line = line_buf.read_next_line().await;
            dbg!(&line_buf);
            dbg!(&line);
        });

        assert_eq!(
            32,
            line_buf.buffer.len() + "This is a simple test.".len(),
            "The buffer should not grow more than it needs to grow to hold the content."
        );
    }

    #[test]
    fn read_next_line_gives_single_line_when_low_capacity() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let mut line_buf = AsyncLineBufferBuilder::new(bytes_reader)
            .with_initial_capacity(8)
            .build();

        async_std::task::block_on(async {
            let line = line_buf.read_next_line().await.unwrap();
            dbg!(&line_buf);
            dbg!(&line);

            assert_eq!("This is a simple test.".as_bytes(), line.as_slice());
        });
    }

    #[test]
    fn read_next_line_gives_single_line_when_high_capacity() {
        let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

        let mut line_buf = AsyncLineBufferBuilder::new(bytes_reader)
            .with_initial_capacity(128)
            .build();

        async_std::task::block_on(async {
            let line = line_buf.read_next_line().await.unwrap();
            dbg!(&line_buf);
            dbg!(&line);

            assert_eq!("This is a simple test.".as_bytes(), line.as_slice());
        });
    }

    #[test]
    fn read_next_line_gives_first_line_when_multiple_lines() {
        let bytes_reader =
            BufReader::new("This is a simple test.\nAnd this is another line.".as_bytes());

        let mut line_buf = AsyncLineBufferBuilder::new(bytes_reader)
            .with_initial_capacity(128)
            .build();

        async_std::task::block_on(async {
            let line = line_buf.read_next_line().await.unwrap();
            dbg!(&line_buf);
            dbg!(&line);

            assert_eq!("This is a simple test.".as_bytes(), line.as_slice());
        });
    }

    #[test]
    fn read_next_line_gives_next_line_when_multiple_lines() {
        let bytes_reader =
            BufReader::new("This is a simple test.\nAnd this is another line.".as_bytes());

        let mut line_buf = AsyncLineBufferBuilder::new(bytes_reader)
            .with_initial_capacity(128)
            .build();

        async_std::task::block_on(async {
            let _ = line_buf.read_next_line().await.unwrap();
            let second_line = line_buf.read_next_line().await.unwrap();
            dbg!(&line_buf);
            dbg!(&second_line);

            assert_eq!(
                "And this is another line.".as_bytes(),
                second_line.as_slice()
            );
        });
    }

    #[test]
    fn read_next_line_reads_many_lines() {
        let bytes_reader = BufReader::new(
            "Hi.\nTwo lines.\nA billion and one lines.\nMany many,\nmany lines.".as_bytes(),
        );

        let mut line_buf = AsyncLineBufferBuilder::new(bytes_reader)
            .with_initial_capacity(8)
            .build();

        async_std::task::block_on(async {
            let line_1 = line_buf.read_next_line().await.unwrap();
            assert_eq!("Hi.".as_bytes(), line_1.as_slice());
            dbg!(line_buf.as_string());

            let line_2 = line_buf.read_next_line().await.unwrap();
            assert_eq!("Two lines.".as_bytes(), line_2.as_slice());
            dbg!(line_buf.as_string());

            let line_3 = line_buf.read_next_line().await.unwrap();
            assert_eq!("A billion and one lines.".as_bytes(), line_3.as_slice());
            dbg!(line_buf.as_string());

            let line_4 = line_buf.read_next_line().await.unwrap();
            assert_eq!("Many many,".as_bytes(), line_4.as_slice());
            dbg!(line_buf.as_string());

            let line_5 = line_buf.read_next_line().await.unwrap();
            assert_eq!("many lines.".as_bytes(), line_5.as_slice());
            dbg!(line_buf.as_string());

            let nonexistant = line_buf.read_next_line().await;
            assert!(nonexistant.is_none());
        });
    }

    #[test]
    fn read_lines_works_when_capacity_stupid_low() {
        let bytes_reader = BufReader::new("This is a simple line.".as_bytes());

        let mut line_buf = AsyncLineBufferBuilder::new(bytes_reader)
            .with_initial_capacity(1)
            .build();

        async_std::task::block_on(async {
            let line = line_buf.read_next_line().await.unwrap();
            assert_eq!("This is a simple line.".as_bytes(), line.as_slice());
        });
    }

    #[test]
    fn read_lines_works_when_capacity_stupid_low_multiple_lines() {
        let bytes_reader = BufReader::new(
            "This is a simple line.\nAnd this is a second line.\nAnd this is a third.".as_bytes(),
        );

        let mut line_buf = AsyncLineBufferBuilder::new(bytes_reader)
            .with_initial_capacity(1)
            .build();

        async_std::task::block_on(async {
            let line = line_buf.read_next_line().await.unwrap();
            assert_eq!("This is a simple line.".as_bytes(), line.as_slice());

            let line = line_buf.read_next_line().await.unwrap();
            assert_eq!("And this is a second line.".as_bytes(), line.as_slice());

            let line = line_buf.read_next_line().await.unwrap();
            assert_eq!("And this is a third.".as_bytes(), line.as_slice());
        });
    }

    #[test]
    fn read_lines_works_when_capacity_stupid_low_and_lines_stupid_short() {
        let bytes_reader = BufReader::new("T\nh\nis\na\nt\ne\ns\nt\n.".as_bytes());

        let mut line_buf = AsyncLineBufferBuilder::new(bytes_reader)
            .with_initial_capacity(1)
            .build();

        async_std::task::block_on(async {
            let line = line_buf.read_next_line().await.unwrap();
            assert_eq!("T".as_bytes(), line.as_slice());

            let line = line_buf.read_next_line().await.unwrap();
            assert_eq!("h".as_bytes(), line.as_slice());

            let line = line_buf.read_next_line().await.unwrap();
            assert_eq!("is".as_bytes(), line.as_slice());

            let line = line_buf.read_next_line().await.unwrap();
            assert_eq!("a".as_bytes(), line.as_slice());

            let line = line_buf.read_next_line().await.unwrap();
            assert_eq!("t".as_bytes(), line.as_slice());

            let line = line_buf.read_next_line().await.unwrap();
            assert_eq!("e".as_bytes(), line.as_slice());
        });
    }

    //     #[test]
    //     fn next_write_pos_is_correct() {
    //         let bytes = vec![0u8; 60];

    //         let mut small_buf = LineBufferBuilder::new(bytes.as_slice())
    //             .with_min_capacity(8)
    //             .build();

    //         let mut big_buff = LineBufferBuilder::new(bytes.as_slice())
    //             .with_min_capacity(1024)
    //             .build();

    //         async_std::task::block_on(async {
    //             small_buf.perform_single_read().await;

    //             assert_eq!(8, small_buf.next_write_pos());

    //             big_buff.perform_single_read().await;

    //             assert_eq!(60, big_buff.next_write_pos());
    //         });
    //     }

    //     #[test]
    //     fn perform_single_read_gives_true_if_more_content() {
    //         let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(8)
    //             .build();

    //         async_std::task::block_on(async {
    //             let reader_has_more = line_buf.perform_single_read().await;

    //             assert!(
    //                 reader_has_more,
    //                 "There is still more to read from the reader."
    //             );
    //         });
    //     }

    //     #[test]
    //     fn perform_single_read_gives_false_if_no_more_content() {
    //         let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(8)
    //             .build();

    //         async_std::task::block_on(async {
    //             line_buf.perform_single_read().await;
    //             line_buf.perform_single_read().await;

    //             // After a third read, the entire reader should have been consumed.
    //             let reader_has_more = line_buf.perform_single_read().await;

    //             assert!(
    //                 !reader_has_more,
    //                 "There should not have been more content in the reader."
    //             );
    //         });
    //     }

    //     #[test]
    //     fn last_written_slice_is_correct() {
    //         let bytes_reader =
    //             BufReader::new("This is a simple test. With extra characters.".as_bytes());

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(8)
    //             .build();

    //         async_std::task::block_on(async {
    //             // We'll read twice.
    //             line_buf.perform_single_read().await;
    //             line_buf.perform_single_read().await;

    //             let slice = line_buf.last_written_slice();

    //             assert_eq!(
    //                 "a simple".as_bytes(),
    //                 slice,
    //                 "Expected the contents of the last write."
    //             );
    //         });
    //     }

    //     // #[test]
    //     // fn buffer_completes_after_consuming_entire_reader() {
    //     //     let bytes_reader = BufReader::new("This is a simple test.".as_bytes());

    //     //     let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //     //         .with_min_capacity(8)
    //     //         .build();

    //     //     async_std::task::block_on(async {
    //     //         line_buf.perform_single_read().await;

    //     //         // Perform another read, which will require growing the buffer.
    //     //         line_buf.perform_single_read().await;

    //     //         // One more read of this size should finish the entirety of the given reader.
    //     //         line_buf.perform_single_read().await;
    //     //     });

    //     //     let end_pos = line_buf.last_write_end_pos();
    //     //     let buffer_content = &line_buf.buffer[..end_pos];

    //     //     assert_eq!(
    //     //         buffer_content,
    //     //         "This is a simple test.".as_bytes(),
    //     //         "The content of the buffer should now be the exact value of the input bytes."
    //     //     );
    //     // }

    //     #[test]
    //     fn read_next_line_consumes_remaining_reader() {
    //         let bytes = "This is a simple test.".as_bytes();
    //         let bytes_reader = BufReader::new(bytes);

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(8)
    //             .build();

    //         async_std::task::block_on(async {
    //             let line_read = line_buf.read_next_line().await;

    //             if let ReadLineResult::EndOfFile(line) = line_read {
    //                 assert_eq!(
    //                     bytes,
    //                     line.as_slice(),
    //                     "Expected the read content to match the input content."
    //                 );
    //             } else {
    //                 assert!(false, "Expected EndOfFile for the read line result.");
    //             }
    //         });
    //     }

    //     #[test]
    //     fn read_next_line_reads_line_when_below_capacity() {
    //         let bytes_reader = BufReader::new(
    //             "This is a simple test.\nAnd this is another line in the test.".as_bytes(),
    //         );

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(1024)
    //             .build();

    //         async_std::task::block_on(async {
    //             let line_read = line_buf.read_next_line().await;

    //             assert_eq!(
    //                 line_read.expect_continue().as_slice(),
    //                 "This is a simple test.".as_bytes(),
    //                 "Expected the read content to match the input content."
    //             );
    //         });
    //     }

    //     #[test]
    //     fn try_drain_resulting_line_gives_correct_result_when_enough_capacity() {
    //         let bytes_reader = BufReader::new(
    //             "This is a simple test.\nAnd this is another line in the test.".as_bytes(),
    //         );

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(1024)
    //             .build();

    //         async_std::task::block_on(async {
    //             line_buf.perform_single_read().await;

    //             let drained = line_buf
    //                 .try_drain_line()
    //                 .expect("Must have the given line.");

    //             assert_eq!("This is a simple test.".as_bytes(), drained.as_slice());
    //         });
    //     }

    //     #[test]
    //     fn try_drain_resulting_line_gives_correct_result_when_not_enough_capacity() {
    //         let bytes_reader = BufReader::new(
    //             "This is a simple test.\nAnd this is another line in the test.".as_bytes(),
    //         );

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(8)
    //             .build();

    //         async_std::task::block_on(async {
    //             line_buf.perform_single_read().await;

    //             let drained = line_buf.try_drain_line();

    //             assert!(
    //                 drained.is_none(),
    //                 "One read was not enough to provide a full line with this capacity."
    //             );
    //         });
    //     }

    //     #[test]
    //     fn try_drain_resulting_line_gives_correct_result_after_multiple_reads() {
    //         let bytes_reader = BufReader::new(
    //             "This is a simple test.\nAnd this is another line in the test.".as_bytes(),
    //         );

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(8)
    //             .build();

    //         async_std::task::block_on(async {
    //             // Three reads required to complete the first line.
    //             line_buf.perform_single_read().await;
    //             line_buf.perform_single_read().await;
    //             line_buf.perform_single_read().await;

    //             let drained = line_buf
    //                 .try_drain_line()
    //                 .expect("Must have the given line.");

    //             assert_eq!("This is a simple test.".as_bytes(), drained.as_slice());
    //         });
    //     }

    //     #[test]
    //     fn buffer_has_expected_state_after_draining_line() {
    //         let bytes_reader = BufReader::new(
    //             "This is a simple test.\nAnd this is another line in the test.".as_bytes(),
    //         );

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(128)
    //             .build();

    //         async_std::task::block_on(async {
    //             line_buf.perform_single_read().await;

    //             let drained = line_buf
    //                 .try_drain_line()
    //                 .expect("Must have the given line.");

    //             assert_eq!(37, line_buf.next_write_pos());
    //         });
    //     }

    //     #[test]
    //     fn read_next_line_reads_two_lines() {
    //         let bytes_reader = BufReader::new(
    //             "This is a simple test.\nAnd this is another line in the test.".as_bytes(),
    //         );

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(128)
    //             .build();

    //         async_std::task::block_on(async {
    //             let line_read = line_buf.read_next_line().await;

    //             assert_eq!(
    //                 "This is a simple test.".as_bytes(),
    //                 line_read.expect_continue().as_slice(),
    //                 "Expected the read content to match the input content."
    //             );

    //             let line_read = line_buf.read_next_line().await;

    //             assert_eq!(
    //                 "And this is another line in the test.".as_bytes(),
    //                 line_read.expect_eof().as_slice(),
    //                 "Expected the read content to match the input content."
    //             );
    //         });
    //     }

    //     #[test]
    //     fn read_next_line_reads_three_lines_with_big_buffer() {
    //         let bytes_reader = BufReader::new(
    //             "This is a simple test.\nAnd this is another line in the test.\nAnd this is one last, third line.".as_bytes(),
    //         );

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(1024)
    //             .build();

    //         async_std::task::block_on(async {
    //             let line_read = line_buf.read_next_line().await;

    //             assert_eq!(
    //                 "This is a simple test.".as_bytes(),
    //                 line_read.expect_continue().as_slice(),
    //                 "Expected the read content to match the input content."
    //             );

    //             let line_read = line_buf.read_next_line().await;

    //             assert_eq!(
    //                 "And this is another line in the test.".as_bytes(),
    //                 line_read.expect_continue().as_slice(),
    //                 "Expected the read content to match the input content."
    //             );

    //             let line_read = line_buf.read_next_line().await;

    //             assert_eq!(
    //                 "And this is one last, third line.".as_bytes(),
    //                 line_read.expect_eof().as_slice(),
    //                 "Expected the read content to match the input content."
    //             );
    //         });
    //     }

    //     #[test]
    //     fn read_next_line_reads_three_lines_with_little_buffer() {
    //         let bytes_reader = BufReader::new(
    //             "This is a simple test.\nAnd this is another line in the test.\nAnd this is one last, third line.".as_bytes(),
    //         );

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(8)
    //             .build();

    //         async_std::task::block_on(async {
    //             let line_read = line_buf.read_next_line().await;

    //             assert_eq!(
    //                 "This is a simple test.".as_bytes(),
    //                 line_read.expect_continue().as_slice(),
    //                 "Expected the read content to match the input content."
    //             );

    //             let line_read = line_buf.read_next_line().await;

    //             assert_eq!(
    //                 "And this is another line in the test.".as_bytes(),
    //                 line_read.expect_continue().as_slice(),
    //                 "Expected the read content to match the input content."
    //             );

    //             let line_read = line_buf.read_next_line().await;

    //             assert_eq!(
    //                 "And this is one last, third line.".as_bytes(),
    //                 line_read.expect_eof().as_slice(),
    //                 "Expected the read content to match the input content."
    //             );
    //         });
    //     }

    //     #[test]
    //     fn read_next_line_gives_zero_byte_vec_when_no_more_data() {
    //         let bytes_reader = BufReader::new(
    //             "This is a simple test.\nAnd this is another line in the test.\nAnd this is one last, third line.".as_bytes(),
    //         );

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(8)
    //             .build();

    //         async_std::task::block_on(async {
    //             let _ = line_buf.read_next_line().await;
    //             let _ = line_buf.read_next_line().await;

    //             let line = line_buf.read_next_line().await;

    //             line.expect_eof();
    //         });
    //     }

    //     #[test]
    //     fn buffer_with_stupidly_low_size_still_works() {
    //         let bytes_reader = BufReader::new(
    //             "This is a simple test.\nAnd this is another line in the test.\nAnd this is one last, third line.".as_bytes(),
    //         );

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(2)
    //             .build();

    //         async_std::task::block_on(async {
    //             let line1 = line_buf.read_next_line().await.expect_continue();
    //             let line2 = line_buf.read_next_line().await.expect_continue();
    //             let line3 = line_buf.read_next_line().await.expect_eof();

    //             assert_eq!("This is a simple test.".as_bytes(), line1.as_slice());
    //             assert_eq!(
    //                 "And this is another line in the test.".as_bytes(),
    //                 line2.as_slice()
    //             );
    //             assert_eq!(
    //                 "And this is one last, third line.".as_bytes(),
    //                 line3.as_slice()
    //             );
    //         });
    //     }

    //     #[test]
    //     fn content_ending_with_newline() {
    //         let bytes_reader = BufReader::new(
    //             "This is a simple test.\nAnd this is another line in the test.\nAnd this is one last, third line.\n".as_bytes(),
    //         );

    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(16)
    //             .build();

    //         async_std::task::block_on(async {
    //             let line1 = line_buf.read_next_line().await.expect_continue();
    //             let line2 = line_buf.read_next_line().await.expect_continue();
    //             dbg!(&line_buf);
    //             let line3 = line_buf.read_next_line().await.expect_continue();
    //             let line4 = line_buf.read_next_line().await.expect_eof();

    //             dbg!(&line_buf);
    //             assert!(false);

    //             assert_eq!("This is a simple test.".as_bytes(), line1.as_slice());
    //             assert_eq!(
    //                 "And this is another line in the test.".as_bytes(),
    //                 line2.as_slice()
    //             );
    //             assert_eq!(
    //                 "And this is one last, third line.".as_bytes(),
    //                 line3.as_slice()
    //             );
    //         });
    //     }

    //     fn shakespeare() {
    //         let hamlet_txt = "
    //         ACT I
    // SCENE I. Elsinore. A platform before the castle.

    //     FRANCISCO at his post. Enter to him BERNARDO

    // BERNARDO

    //     Who's there?

    // FRANCISCO

    //     Nay, answer me: stand, and unfold yourself.

    // BERNARDO

    //     Long live the king!

    // FRANCISCO

    //     Bernardo?

    // BERNARDO

    //     He.

    // FRANCISCO

    //     You come most carefully upon your hour.

    // BERNARDO

    //     'Tis now struck twelve; get thee to bed, Francisco.

    // FRANCISCO

    //     For this relief much thanks: 'tis bitter cold,
    //     And I am sick at heart.

    // BERNARDO

    //     Have you had quiet guard?

    // FRANCISCO

    //     Not a mouse stirring.

    // BERNARDO

    //     Well, good night.
    //     If you do meet Horatio and Marcellus,
    //     The rivals of my watch, bid them make haste.
    //         ";

    //         let bytes_reader = BufReader::new(hamlet_txt.as_bytes());
    //         let mut line_buf = LineBufferBuilder::new(bytes_reader)
    //             .with_min_capacity(16)
    //             .build();
    //     }
}

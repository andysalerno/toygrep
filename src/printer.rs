/// A trait describing the ability to "send" a message to a printer.
pub(crate) trait PrinterSender: Clone {
    fn send(&self, message: PrintMessage);
}

/// A result that can be printed by a printer.
#[derive(Debug, Clone)]
pub(crate) struct PrintableResult {
    target_name: String,
    line_num: usize,
    text: Vec<u8>,
}

impl PrintableResult {
    pub(crate) fn new(target_name: String, line_num: usize, text: Vec<u8>) -> Self {
        Self {
            target_name,
            line_num,
            text,
        }
    }

    fn text_as_string(self) -> String {
        String::from_utf8(self.text).expect("Failed parsing text to utf8 in PrintableResult.")
    }
}

/// A message that can be sent to a printer for printing.
#[derive(Debug, Clone)]
pub(crate) enum PrintMessage {
    Printable(PrintableResult),

    /// Signals to the printer that there will be no more messages for the named target.
    EndOfReading {
        target_name: String,
    },
}

mod blocking_printer {
    use super::*;

    /// A printer sender that "sends" by simply printint immediately
    /// (and blocking while doing so).
    #[derive(Debug, Clone)]
    struct BlockingPrinterSender;

    impl PrinterSender for BlockingPrinterSender {
        fn send(&self, message: PrintMessage) {
            println!("{:?}", message);
        }
    }
}

pub(crate) mod threaded_printer {
    use super::*;
    use crate::matcher::Matcher;
    use std::collections::HashMap;
    use std::io::Write;
    use std::sync::mpsc;
    use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

    #[derive(Clone)]
    pub(crate) struct ThreadedPrinterSender {
        sender: mpsc::Sender<PrintMessage>,
    }

    impl ThreadedPrinterSender {
        pub(crate) fn new(sender: mpsc::Sender<PrintMessage>) -> Self {
            Self { sender }
        }
    }

    impl PrinterSender for ThreadedPrinterSender {
        fn send(&self, mut message: PrintMessage) {
            self.sender.send(message).expect("Failed sending message.");
        }
    }

    struct ThreadedPrinterConfig {
        print_line_num: bool,
        group_by_target: bool,
    }

    pub(crate) struct ThreadedPrinterBuilder<M: Matcher> {
        config: ThreadedPrinterConfig,
        receiver: mpsc::Receiver<PrintMessage>,
        matcher: Option<M>,
    }

    impl<M: Matcher> ThreadedPrinterBuilder<M> {
        pub(crate) fn new(receiver: mpsc::Receiver<PrintMessage>) -> Self {
            Self {
                config: ThreadedPrinterConfig {
                    print_line_num: true,
                    group_by_target: true,
                },
                receiver,
                matcher: None,
            }
        }

        pub(crate) fn group_by_target(mut self, should_group: bool) -> Self {
            self.config.group_by_target = should_group;
            self
        }

        pub(crate) fn with_matcher(mut self, matcher: M) -> Self {
            self.matcher = Some(matcher);
            self
        }

        pub(crate) fn build(self) -> ThreadedPrinter<M> {
            ThreadedPrinter::new(self.matcher, self.receiver, self.config)
        }
    }

    /// A simple printer that is just a proxy to the println! macro.
    pub(crate) struct ThreadedPrinter<M: Matcher> {
        config: ThreadedPrinterConfig,
        receiver: mpsc::Receiver<PrintMessage>,
        file_to_matches: HashMap<String, Vec<PrintableResult>>,
        matcher: Option<M>,
    }

    impl<M: Matcher> ThreadedPrinter<M> {
        fn new(
            matcher: Option<M>,
            receiver: mpsc::Receiver<PrintMessage>,
            config: ThreadedPrinterConfig,
        ) -> Self {
            Self {
                receiver,
                config,
                file_to_matches: HashMap::new(),
                matcher,
            }
        }

        pub(crate) fn listen(&mut self) {
            while let Ok(message) = self.receiver.recv() {
                if self.config.group_by_target {
                    match message {
                        PrintMessage::Printable(printable) => {
                            if self.file_to_matches.get(&printable.target_name).is_none() {
                                self.file_to_matches
                                    .insert(printable.target_name.clone(), Vec::new());
                            }

                            let line_results = self
                                .file_to_matches
                                .get_mut(&printable.target_name)
                                .unwrap();
                            line_results.push(printable);
                        }
                        PrintMessage::EndOfReading { target_name } => {
                            self.print_target_results(&target_name);
                        }
                    }
                } else if let PrintMessage::Printable(printable) = message {
                    self.print_line_result(printable);
                }
            }
        }

        fn print_target_results(&mut self, name: &str) {
            let matches_for_target = self
                .file_to_matches
                .remove(name)
                .unwrap_or_else(|| panic!("Target {} was never specified.", name));

            println!("\n{}", name);
            for printable in matches_for_target {
                self.print_line_result(printable);
            }
        }

        fn print_line_result(&self, printable: PrintableResult) {
            let line_num = if self.config.print_line_num {
                format!("{}:", printable.line_num)
            } else {
                "".to_owned()
            };

            if let Some(matcher) = &self.matcher {
                ThreadedPrinter::print_colorized(&line_num, matcher, &printable.text);
            } else {
                print!("{}{}", line_num, printable.text_as_string());
            }
        }

        fn print_colorized(line_num_chunk: &str, matcher: &M, text: &[u8]) {
            let mut stdout = StandardStream::stdout(ColorChoice::Always);

            // First, write the line num in green.
            stdout
                .set_color(ColorSpec::new().set_fg(Some(Color::Green)))
                .expect("Failed setting color.");

            write!(&mut stdout, "{}", line_num_chunk).expect("Failed writing line num chunk.");

            stdout.reset().expect("Failed to reset stdout color.");

            let mut start = 0;
            for match_range in matcher.find_matches(text) {
                let until_match = &text[start..match_range.start];
                let during_match = &text[match_range.start..match_range.stop];

                write!(
                    &mut stdout,
                    "{}",
                    std::str::from_utf8(until_match).expect("Invalid utf8 during colorization")
                )
                .expect("Failure writing to stdout");

                stdout
                    .set_color(ColorSpec::new().set_fg(Some(Color::Green)))
                    .expect("Failed setting color.");
                write!(
                    &mut stdout,
                    "{}",
                    std::str::from_utf8(during_match).expect("Invalid utf8 during colorization")
                )
                .expect("Failure writing to stdout");

                stdout.reset().expect("Failed to reset stdout color.");

                start = match_range.stop;
            }

            // print remainder after final match
            let remainder = &text[start..];
            write!(
                &mut stdout,
                "{}",
                std::str::from_utf8(remainder).expect("Invalid utf8 during colorization")
            )
            .expect("Failure writing to stdout");
        }
    }
}

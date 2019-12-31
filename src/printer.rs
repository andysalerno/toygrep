use crate::error::{Error, Result};
use std::thread;

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

    /// Consume `self` and convert the `text` into a utf8 `String`.
    fn text_as_string(self) -> Result<String> {
        let target_name = self.target_name;
        String::from_utf8(self.text).map_err(|_| Error::Utf8PrintFail(target_name))
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

    /// Simply a string for displaying.
    Display(String),
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
        fn send(&self, message: PrintMessage) {
            self.sender.send(message).expect("Failed sending message.");
        }
    }

    struct ThreadedPrinterConfig {
        print_line_num: bool,
        group_by_target: bool,
        print_immediately: bool,
    }

    pub(crate) struct ThreadedPrinterBuilder<M: Matcher> {
        config: ThreadedPrinterConfig,
        receiver: mpsc::Receiver<PrintMessage>,
        matcher: Option<M>,
    }

    impl<M: Matcher + 'static> ThreadedPrinterBuilder<M> {
        pub(crate) fn new(receiver: mpsc::Receiver<PrintMessage>) -> Self {
            Self {
                config: ThreadedPrinterConfig {
                    print_line_num: true,
                    group_by_target: true,
                    print_immediately: false,
                },
                receiver,
                matcher: None,
            }
        }

        pub(crate) fn group_by_target(mut self, should_group: bool) -> Self {
            self.config.group_by_target = should_group;
            self
        }

        pub(crate) fn print_immediately(mut self, should_print_immediately: bool) -> Self {
            self.config.print_immediately = should_print_immediately;
            self
        }

        pub(crate) fn with_matcher(mut self, matcher: M) -> Self {
            self.matcher = Some(matcher);
            self
        }

        pub(crate) fn build(self) -> ThreadedPrinter<M> {
            assert!(
                !(self.config.print_immediately && self.config.group_by_target),
                "The current configuration is not valid -- both 'print immediately' and \
                 'group by target' features are enabled, but when 'print immediately' \
                 is configured, 'I can't also group by target'."
            );
            ThreadedPrinter::new(self.matcher, self.receiver, self.config)
        }
    }

    /// A simple printer that can be spawned on a separate thread.
    pub(crate) struct ThreadedPrinter<M: Matcher> {
        config: ThreadedPrinterConfig,
        receiver: mpsc::Receiver<PrintMessage>,
        file_to_matches: HashMap<String, Vec<PrintableResult>>,
        matcher: Option<M>,
    }

    impl<M: Matcher + 'static> ThreadedPrinter<M> {
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

        pub(crate) fn spawn(mut self) -> thread::JoinHandle<()> {
            thread::spawn(move || self.listen())
        }

        pub(crate) fn listen(&mut self) {
            let stdout = StandardStream::stdout(ColorChoice::Auto);
            let mut stdout = stdout.lock();

            let mut errors = Vec::new();

            while let Ok(message) = self.receiver.recv() {
                if self.config.group_by_target {
                    match message {
                        PrintMessage::Display(msg) => {
                            write!(&mut stdout, "{}", msg).expect("Failed writing tou stdout.");
                        }
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
                            let _ = self
                                .print_target_results(&mut stdout, &target_name)
                                .map_err(|e| errors.push(e));
                        }
                    }
                } else if let PrintMessage::Printable(printable) = message {
                    let _ = self
                        .print_line_result(&mut stdout, printable)
                        .map_err(|e| errors.push(e));
                }
            }

            errors.into_iter().for_each(|e| match e {
                Error::Utf8PrintFail(target) => {
                    eprintln!("Invalid Utf8 encountered while parsing: {}", target)
                }
                _ => eprintln!("Printer encountered an unknown error: {:?}", e),
            });
        }

        fn print_target_results<W>(&mut self, writer: &mut W, name: &str) -> Result<()>
        where
            W: Write + WriteColor,
        {
            // TODO: continue on error and present results in end
            let matches_for_target = self.file_to_matches.remove(name).unwrap_or_default();

            if matches_for_target.is_empty() {
                // Nothing to do.
                return Ok(());
            }

            writeln!(writer, "\n{}", name).expect("Error writing to stdout.");
            for printable in matches_for_target {
                self.print_line_result(writer, printable)?;
            }

            Ok(())
        }

        fn print_line_result<W>(&self, writer: &mut W, printable: PrintableResult) -> Result<()>
        where
            W: Write + WriteColor,
        {
            let line_num = if self.config.print_line_num {
                format!("{}:", printable.line_num)
            } else {
                "".to_owned()
            };

            if let Some(matcher) = &self.matcher {
                ThreadedPrinter::print_colorized(&line_num, matcher, writer, &printable);
            } else {
                write!(writer, "{}{}", line_num, printable.text_as_string()?)
                    .expect("Error writing to stdout.");
            }

            Ok(())
        }

        fn print_colorized<W>(
            line_num_chunk: &str,
            matcher: &M,
            writer: &mut W,
            printable: &PrintableResult,
        ) where
            W: Write + WriteColor,
        {
            let text = &printable.text;

            let parse_utf8 = |bytes| {
                std::str::from_utf8(bytes)
                    .map_err(|_| Error::Utf8PrintFail(printable.target_name.to_owned()))
            };

            // First, write the line num in green.
            writer
                .set_color(ColorSpec::new().set_fg(Some(Color::Green)))
                .expect("Failed setting color.");

            write!(writer, "{}", line_num_chunk).expect("Failed writing line num chunk.");

            // Then, reset color to print the non-matching segment.
            writer.reset().expect("Failed to reset stdout color.");

            let mut start = 0;
            for match_range in matcher.find_matches(text) {
                let until_match = &text[start..match_range.start];
                let during_match = &text[match_range.start..match_range.stop];

                if let Ok(text) = parse_utf8(until_match) {
                    write!(writer, "{}", text).expect("Failure writing to stdout");
                } else {
                    eprintln!("Utf8 parsing error for target: {}", printable.target_name);
                }

                // The match itself is printed in red.
                // stdout
                writer
                    .set_color(ColorSpec::new().set_fg(Some(Color::Red)))
                    .expect("Failed setting color.");

                if let Ok(text) = parse_utf8(during_match) {
                    write!(writer, "{}", text).expect("Failure writing to stdout");
                } else {
                    eprintln!("Utf8 parsing error for target: {}", printable.target_name);
                }

                writer.reset().expect("Failed to reset stdout color.");

                start = match_range.stop;
            }

            // print remainder after final match
            let remainder = &text[start..];

            if let Ok(text) = parse_utf8(remainder) {
                write!(writer, "{}", text).expect("Failure writing to stdout");
            } else {
                eprintln!("Utf8 parsing error for target: {}", printable.target_name);
            }
        }
    }
}

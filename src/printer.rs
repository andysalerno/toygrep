use crate::error::{Error, Result};
use crate::matcher::Matcher;
use crate::time_log::TimeLog;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

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

/// A builder for a printer sender, which may be either blocking
/// or non-blocking (threaded).
pub(crate) struct Printer<M: Matcher> {
    config: PrettyPrinter::Config,
    receiver: mpsc::Receiver<PrintMessage>,
    matcher: Option<M>,
}

impl<M: Matcher + 'static> Printer<M> {
    pub(crate) fn new(receiver: mpsc::Receiver<PrintMessage>) -> Self {
        Self {
            config: PrettyPrinter::Config {
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

    pub(crate) fn build_blocking(self) -> ThreadedPrinter<M> {
        assert!(
            !(self.config.print_immediately && self.config.group_by_target),
            "The current configuration is not valid -- both 'print immediately' and \
             'group by target' features are enabled, but when 'print immediately' \
             is configured, 'I can't also group by target'."
        );
        ThreadedPrinter::new(self.matcher, self.receiver, self.config)
    }

    pub(crate) fn spawn_threaded(receiver: mpsc::Receiver<PrintMessage>) -> ThreadedPrinterSender {
        todo!()
    }
}

/// This module contains the types and logic
/// for a printer that can group lines
/// and color matching patterns.
///
/// It is not exposed outside this module,
/// but module `threaded_printer` contains a
/// threaded wrapper, and module `blocking_printer`
/// contains a blocking wrapper that can be
/// used externally.
mod pretty_printer {
    use super::*;
    use crate::matcher::Matcher;
    use std::collections::HashMap;
    use std::io::Write;
    use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

    pub(super) struct Config {
        print_line_num: bool,
        group_by_target: bool,
        print_immediately: bool,
    }

    pub(super) struct Printer<M: Matcher> {
        file_to_matches: HashMap<String, Vec<PrintableResult>>,
        config: Config,
        matcher: Option<M>,
    }

    impl<M: Matcher> Printer<M> {
        pub(super) fn new(matcher: Option<M>, config: Config) -> Self {
            Self {
                matcher,
                config,
                file_to_matches: HashMap::new(),
            }
        }

        pub(super) fn print<W>(&mut self, mut writer: W, message: PrintMessage)
        where
            W: Write + WriteColor,
        {
            if self.config.group_by_target {
                match message {
                    PrintMessage::Display(msg) => {
                        print!("{}", msg);
                    }
                    PrintMessage::Printable(printable) => {
                        let line_results = self
                            .file_to_matches
                            .entry(printable.target_name.to_owned())
                            .or_default();

                        line_results.push(printable);
                    }
                    PrintMessage::EndOfReading { target_name } => {
                        let _ = self.print_target_results(&mut writer, &target_name);
                    }
                }
            } else if let PrintMessage::Printable(printable) = message {
                let _ = self.print_line_result(&mut writer, printable);
            }
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
                Printer::print_colorized(&line_num, matcher, writer, &printable);
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

pub(crate) mod blocking_printer {
    use super::*;

    struct Sender<M: Matcher> {
        pretty_printer: pretty_printer::Printer<M>,
    }
}

pub(crate) mod threaded_printer {
    use super::*;
    use crate::matcher::Matcher;
    use std::sync::mpsc;
    use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

    #[derive(Clone)]
    pub(crate) struct Sender {
        sender: mpsc::Sender<PrintMessage>,
    }

    impl Sender {
        pub(crate) fn new(sender: mpsc::Sender<PrintMessage>) -> Self {
            Self { sender }
        }
    }

    impl PrinterSender for Sender {
        fn send(&self, message: PrintMessage) {
            self.sender.send(message).expect("Failed sending message.");
        }
    }

    /// A simple printer that can be spawned on a separate thread,
    /// and receive messages to print from the `Sender`.
    pub(crate) struct Printer<M: Matcher> {
        receiver: mpsc::Receiver<PrintMessage>,
        printer: pretty_printer::Printer<M>,
    }

    impl<M: Matcher + 'static> Printer<M> {
        fn new(
            matcher: Option<M>,
            receiver: mpsc::Receiver<PrintMessage>,
            config: pretty_printer::Config,
        ) -> Self {
            Self {
                receiver,
                printer: pretty_printer::Printer::new(matcher, config),
            }
        }

        pub(super) fn spawn(mut self) -> thread::JoinHandle<TimeLog> {
            thread::spawn(move || self.listen())
        }

        pub(super) fn listen(&mut self) -> TimeLog {
            let stdout = StandardStream::stdout(ColorChoice::Auto);
            let mut stdout = stdout.lock();

            // At first, the instant represents 'spawn-to-first-print'.
            let spawn_to_print_instant = Instant::now();
            // Bit of a hack -- not using `instant` the intended way here.
            let mut time_log = TimeLog::new(spawn_to_print_instant);

            while let Ok(message) = self.receiver.recv() {
                self.printer.print(&mut stdout, message);
            }

            time_log
        }
    }
}

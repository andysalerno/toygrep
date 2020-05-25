mod blocking_printer;
mod printer;
mod threaded_printer;
mod null_printer;

use crate::error::{Error, Result};
use crate::matcher::Matcher;
use crate::time_log::TimeLog;
use crossbeam_channel::unbounded;
use printer::PrettyPrinter;
use std::thread;

/// A trait describing the ability to "send" a message to a printer.
pub(crate) trait PrinterSender: Clone + Send {
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

/// Config values used internally to construct a printer.
struct Config {
    print_line_num: bool,
    group_by_target: bool,
    print_immediately: bool,
}

/// A builder for a printer sender, which may be either blocking
/// or non-blocking (threaded).
pub(crate) struct Printer<M: Matcher> {
    config: Config,
    matcher: Option<M>,
}

impl<M: Matcher + Sync + 'static> Printer<M> {
    pub(crate) fn make_null(self) -> impl PrinterSender {
        null_printer::NullPrinter
    }

    pub(crate) fn new() -> Self {
        Self {
            config: Config {
                print_line_num: true,
                group_by_target: true,
                print_immediately: false,
            },
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

    pub(crate) fn build_blocking(self) -> impl PrinterSender {
        blocking_printer::BlockingSender::new(PrettyPrinter::new(self.matcher, self.config))
    }

    pub(crate) fn spawn_threaded(self) -> (impl PrinterSender, std::thread::JoinHandle<TimeLog>) {
        let (sender, receiver) = unbounded();
        let sender = crate::print::threaded_printer::Sender::new(sender);
        let mut printer =
            crate::print::threaded_printer::Printer::new(self.matcher, receiver, self.config);

        (sender, thread::spawn(move || printer.listen()))
    }
}

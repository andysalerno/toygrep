use super::{Config, PrettyPrinter, PrintMessage, PrinterSender};
use crate::matcher::Matcher;
use crate::time_log::TimeLog;
use crossbeam_channel::{Receiver as ChannelReceiver, Sender as ChannelSender};
use std::time::Instant;
use termcolor::{ColorChoice, StandardStream};

#[derive(Clone)]
pub(crate) struct Sender {
    sender: ChannelSender<PrintMessage>,
}

impl Sender {
    pub(super) fn new(sender: ChannelSender<PrintMessage>) -> Self {
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
pub(super) struct Printer<M: Matcher> {
    receiver: ChannelReceiver<PrintMessage>,
    printer: PrettyPrinter<M>,
}

impl<M: Matcher> Printer<M> {
    pub(super) fn new(
        matcher: Option<M>,
        receiver: ChannelReceiver<PrintMessage>,
        config: Config,
    ) -> Self {
        Self {
            receiver,
            printer: PrettyPrinter::new(matcher, config),
        }
    }

    pub(super) fn listen(&mut self) -> TimeLog {
        let stdout = StandardStream::stdout(ColorChoice::Auto);
        let mut stdout = stdout.lock();

        // At first, the instant represents 'spawn-to-first-print'.
        let spawn_to_print_instant = Instant::now();
        let mut time_log = TimeLog::new(spawn_to_print_instant);
        let mut has_logged = false;

        while let Ok(message) = self.receiver.recv() {
            if !has_logged {
                time_log.log_printer_spawn_to_print();
                has_logged = true;
            }

            self.printer.print(&mut stdout, message);
        }

        time_log.log_print_duration();
        time_log
    }
}

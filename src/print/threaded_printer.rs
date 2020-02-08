use super::{Config, PrettyPrinter, PrintMessage, PrinterSender};
use crate::matcher::Matcher;
use crate::time_log::TimeLog;
use std::sync::mpsc;
use std::time::Instant;
use termcolor::{ColorChoice, StandardStream};

#[derive(Clone)]
pub(crate) struct Sender {
    sender: mpsc::Sender<PrintMessage>,
}

impl Sender {
    pub(super) fn new(sender: mpsc::Sender<PrintMessage>) -> Self {
        Self { sender }
    }
}

impl PrinterSender for Sender {
    fn send(&self, message: PrintMessage) {
        dbg!("Sending print message.");
        self.sender.send(message).expect("Failed sending message.");
    }
}

/// A simple printer that can be spawned on a separate thread,
/// and receive messages to print from the `Sender`.
pub(super) struct Printer<M: Matcher> {
    receiver: mpsc::Receiver<PrintMessage>,
    printer: PrettyPrinter<M>,
}

impl<M: Matcher + 'static> Printer<M> {
    pub(super) fn new(
        matcher: Option<M>,
        receiver: mpsc::Receiver<PrintMessage>,
        config: Config,
    ) -> Self {
        Self {
            receiver,
            printer: PrettyPrinter::new(matcher, config),
        }
    }

    pub(super) fn listen(&mut self) -> TimeLog {
        let mut stdout = StandardStream::stdout(ColorChoice::Auto);
        // let mut stdout = stdout.lock();

        // At first, the instant represents 'spawn-to-first-print'.
        let spawn_to_print_instant = Instant::now();
        // Bit of a hack -- not using `instant` the intended way here.
        let mut time_log = TimeLog::new(spawn_to_print_instant);

        while let Ok(message) = self.receiver.recv() {
            dbg!("Received print message");
            self.printer.print(&mut stdout, message);
        }

        time_log
    }
}

use super::{PrettyPrinter, PrintMessage};
use crate::matcher::Matcher;
use std::sync::Arc;
use std::sync::Mutex;
use termcolor::{ColorChoice, StandardStream};

#[derive(Clone)]
pub(super) struct BlockingSender<M: Matcher + Send + Sync>(Arc<Mutex<PrettyPrinter<M>>>);

impl<M: Matcher + Send + Sync> BlockingSender<M> {
    pub(super) fn new(printer: PrettyPrinter<M>) -> Self {
        BlockingSender(Arc::new(Mutex::new(printer)))
    }
}

impl<M: Matcher + Send + Sync> super::PrinterSender for BlockingSender<M> {
    fn send(&self, message: PrintMessage) {
        // TODO: store stdout in struct
        let mut stdout = StandardStream::stdout(ColorChoice::Auto);
        let mut lock = self.0.lock().expect("Unable to acquire lock.");
        lock.print(&mut stdout, message);
    }
}

use super::{PrintMessage, PrinterSender};

#[derive(Clone)]
pub(super) struct NullPrinter;

impl PrinterSender for NullPrinter {
    fn send(&self, _message: PrintMessage) {
        // Do nothing at all.
    }

}
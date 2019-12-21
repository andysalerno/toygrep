use std::sync::mpsc;

struct StdOutPrinterConfig {}

pub(crate) struct StdOutPrinterBuilder {
    config: StdOutPrinterConfig,
}

impl StdOutPrinterBuilder {
    fn new() -> Self {
        Self {
            config: StdOutPrinterConfig {},
        }
    }
}

/// A simple printer that is just a proxy to the println! macro.
pub(crate) struct StdOutPrinter {
    // config: StdOutPrinterConfig,
    receiver: mpsc::Receiver<String>,
}

impl StdOutPrinter {
    pub fn new(receiver: mpsc::Receiver<String>) -> Self {
        Self { receiver }
    }

    pub fn listen(&self) {
        while let Ok(s) = self.receiver.recv() {
            print!("{}", s);
        }
    }
}

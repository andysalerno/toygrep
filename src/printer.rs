use crate::async_line_buffer::LineResult;
use std::sync::mpsc;

struct StdOutPrinterConfig {
    print_line_num: bool,
    group_by_file: bool,
}

pub(crate) struct StdOutPrinterBuilder {
    config: StdOutPrinterConfig,
    receiver: mpsc::Receiver<LineResult>,
}

impl StdOutPrinterBuilder {
    pub fn new(receiver: mpsc::Receiver<LineResult>) -> Self {
        Self {
            config: StdOutPrinterConfig {
                print_line_num: true,
                group_by_file: true,
            },
            receiver,
        }
    }

    pub fn build(self) -> StdOutPrinter {
        StdOutPrinter::new(self.receiver, self.config)
    }
}

/// A simple printer that is just a proxy to the println! macro.
pub(crate) struct StdOutPrinter {
    config: StdOutPrinterConfig,
    receiver: mpsc::Receiver<LineResult>,
}

impl StdOutPrinter {
    fn new(receiver: mpsc::Receiver<LineResult>, config: StdOutPrinterConfig) -> Self {
        Self { receiver, config }
    }

    pub fn listen(&self) {
        while let Ok(s) = self.receiver.recv() {
            print!("{}", s.text());
        }
    }
}

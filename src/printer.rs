use crate::async_line_buffer::LineResult;
use std::collections::HashMap;
use std::sync::mpsc;

pub(crate) struct PrintableResult {
    pub(crate) file_name: String,
    pub(crate) line_result: LineResult,
}

struct StdOutPrinterConfig {
    print_line_num: bool,
    group_by_file: bool,
}

pub(crate) struct StdOutPrinterBuilder {
    config: StdOutPrinterConfig,
    receiver: mpsc::Receiver<PrintableResult>,
}

impl StdOutPrinterBuilder {
    pub fn new(receiver: mpsc::Receiver<PrintableResult>) -> Self {
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
    receiver: mpsc::Receiver<PrintableResult>,
    file_to_matches: HashMap<String, Vec<LineResult>>,
}

impl StdOutPrinter {
    fn new(receiver: mpsc::Receiver<PrintableResult>, config: StdOutPrinterConfig) -> Self {
        Self {
            receiver,
            config,
            file_to_matches: HashMap::new(),
        }
    }

    pub fn listen(&mut self) {
        while let Ok(s) = self.receiver.recv() {
            if self.config.group_by_file {
                if self.file_to_matches.get(&s.file_name).is_none() {
                    self.file_to_matches.insert(s.file_name.clone(), Vec::new());
                }

                let line_results = self.file_to_matches.get_mut(&s.file_name).unwrap();
                line_results.push(s.line_result);
            } else {
                let line_num = if self.config.print_line_num {
                    format!("{}:", s.line_result.line_num())
                } else {
                    "".to_owned()
                };

                print!("{}{}", line_num, s.line_result.text());
            }
        }

        if self.config.group_by_file {
            for m in self.file_to_matches.iter() {
                println!("\n{}", m.0);
                for line_result in m.1 {
                    let line_num = if self.config.print_line_num {
                        format!("{}:", line_result.line_num())
                    } else {
                        "".to_owned()
                    };

                    print!("{}{}", line_num, line_result.text());
                }
            }
        }
    }
}

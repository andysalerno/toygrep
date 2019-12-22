use crate::async_line_buffer::LineResult;
use std::collections::HashMap;
use std::sync::mpsc;

pub(crate) struct PrintableResult {
    pub(crate) target_name: String,
    pub(crate) line_result: LineResult,
}

struct StdOutPrinterConfig {
    print_line_num: bool,
    group_by_target: bool,
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
                group_by_target: true,
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
            if self.config.group_by_target {
                if self.file_to_matches.get(&s.target_name).is_none() {
                    self.file_to_matches
                        .insert(s.target_name.clone(), Vec::new());
                }

                let line_results = self.file_to_matches.get_mut(&s.target_name).unwrap();
                line_results.push(s.line_result);
            } else {
                let line_num = if self.config.print_line_num {
                    format!("{}:", s.line_result.line_num())
                } else {
                    "".to_owned()
                };

                print!(
                    "{}{}",
                    line_num,
                    std::str::from_utf8(s.line_result.text()).unwrap()
                );
            }
        }

        if self.config.group_by_target {
            for m in self.file_to_matches.iter() {
                println!("\n{}", m.0);
                for line_result in m.1 {
                    let line_num = if self.config.print_line_num {
                        format!("{}:", line_result.line_num())
                    } else {
                        "".to_owned()
                    };

                    print!(
                        "{}{}",
                        line_num,
                        std::str::from_utf8(line_result.text()).unwrap()
                    );
                }
            }
        }
    }
}

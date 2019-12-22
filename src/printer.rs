use crate::async_line_buffer::LineResult;
use std::collections::HashMap;
use std::sync::mpsc;

pub(crate) enum PrintMessage {
    PrintableResult {
        target_name: String,
        line_result: LineResult,
    },
    EndOfReading {
        target_name: String,
    },
}

struct StdOutPrinterConfig {
    print_line_num: bool,
    group_by_target: bool,
}

pub(crate) struct StdOutPrinterBuilder {
    config: StdOutPrinterConfig,
    receiver: mpsc::Receiver<PrintMessage>,
}

impl StdOutPrinterBuilder {
    pub(crate) fn new(receiver: mpsc::Receiver<PrintMessage>) -> Self {
        Self {
            config: StdOutPrinterConfig {
                print_line_num: true,
                group_by_target: true,
            },
            receiver,
        }
    }

    pub(crate) fn build(self) -> StdOutPrinter {
        StdOutPrinter::new(self.receiver, self.config)
    }
}

/// A simple printer that is just a proxy to the println! macro.
pub(crate) struct StdOutPrinter {
    config: StdOutPrinterConfig,
    receiver: mpsc::Receiver<PrintMessage>,
    file_to_matches: HashMap<String, Vec<LineResult>>,
}

impl StdOutPrinter {
    fn new(receiver: mpsc::Receiver<PrintMessage>, config: StdOutPrinterConfig) -> Self {
        Self {
            receiver,
            config,
            file_to_matches: HashMap::new(),
        }
    }

    pub(crate) fn listen(&mut self) {
        while let Ok(message) = self.receiver.recv() {
            if self.config.group_by_target {
                match message {
                    PrintMessage::PrintableResult {
                        target_name,
                        line_result,
                    } => {
                        if self.file_to_matches.get(&target_name).is_none() {
                            self.file_to_matches.insert(target_name.clone(), Vec::new());
                        }

                        let line_results = self.file_to_matches.get_mut(&target_name).unwrap();
                        line_results.push(line_result);
                    }
                    PrintMessage::EndOfReading { target_name } => {
                        self.print_target_results(&target_name);
                    }
                }
            } else if let PrintMessage::PrintableResult { line_result, .. } = message {
                self.print_line_result(&line_result);
            }
        }
    }

    fn print_target_results(&self, name: &str) {
        let matches_for_target = self
            .file_to_matches
            .get(name)
            .expect("Attempt to print match results for a target that was never specified.");

        println!("\n{}", name);
        for line_result in matches_for_target {
            self.print_line_result(line_result);
        }
    }

    fn print_line_result(&self, line_result: &LineResult) {
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

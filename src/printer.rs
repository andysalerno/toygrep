use crate::async_line_buffer::LineResult;
use crate::matcher::Matcher;
use std::collections::HashMap;
use std::io::Write;
use std::sync::mpsc;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

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

pub(crate) struct StdOutPrinterBuilder<M: Matcher> {
    config: StdOutPrinterConfig,
    receiver: mpsc::Receiver<PrintMessage>,
    matcher: Option<M>,
}

impl<M: Matcher> StdOutPrinterBuilder<M> {
    pub(crate) fn new(receiver: mpsc::Receiver<PrintMessage>) -> Self {
        Self {
            config: StdOutPrinterConfig {
                print_line_num: true,
                group_by_target: true,
            },
            receiver,
            matcher: None,
        }
    }

    pub(crate) fn with_matcher(mut self, matcher: M) -> Self {
        self.matcher = Some(matcher);
        self
    }

    pub(crate) fn build(self) -> StdOutPrinter<M> {
        StdOutPrinter::new(self.matcher, self.receiver, self.config)
    }
}

/// A simple printer that is just a proxy to the println! macro.
pub(crate) struct StdOutPrinter<M: Matcher> {
    config: StdOutPrinterConfig,
    receiver: mpsc::Receiver<PrintMessage>,
    file_to_matches: HashMap<String, Vec<LineResult>>,
    matcher: Option<M>,
}

impl<M: Matcher> StdOutPrinter<M> {
    fn new(
        matcher: Option<M>,
        receiver: mpsc::Receiver<PrintMessage>,
        config: StdOutPrinterConfig,
    ) -> Self {
        Self {
            receiver,
            config,
            file_to_matches: HashMap::new(),
            matcher,
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
            .unwrap_or_else(|| panic!("Target {} was never specified.", name));

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

        if let Some(matcher) = &self.matcher {
            StdOutPrinter::print_colorized(&line_num, matcher, line_result.text());
        } else {
            print!(
                "{}{}",
                line_num,
                std::str::from_utf8(line_result.text()).unwrap()
            );
        }
    }

    fn print_colorized(line_num_chunk: &str, matcher: &M, text: &[u8]) {
        let mut stdout = StandardStream::stdout(ColorChoice::Always);

        // First, write the line num in green.
        stdout
            .set_color(ColorSpec::new().set_fg(Some(Color::Green)))
            .expect("Failed setting color.");

        write!(&mut stdout, "{}", line_num_chunk).expect("Failed writing line num chunk.");

        stdout.reset().expect("Failed to reset stdout color.");

        let mut start = 0;
        for match_range in matcher.find_matches(text) {
            let until_match = &text[start..match_range.start];
            let during_match = &text[match_range.start..match_range.stop];

            write!(
                &mut stdout,
                "{}",
                std::str::from_utf8(until_match).expect("Invalid utf8 during colorization")
            )
            .expect("Failure writing to stdout");

            stdout
                .set_color(ColorSpec::new().set_fg(Some(Color::Green)))
                .expect("Failed setting color.");
            write!(
                &mut stdout,
                "{}",
                std::str::from_utf8(during_match).expect("Invalid utf8 during colorization")
            )
            .expect("Failure writing to stdout");

            stdout.reset().expect("Failed to reset stdout color.");

            start = match_range.stop;
        }

        // print remainder after final match
        let remainder = &text[start..];
        write!(
            &mut stdout,
            "{}",
            std::str::from_utf8(remainder).expect("Invalid utf8 during colorization")
        )
        .expect("Failure writing to stdout");
    }
}

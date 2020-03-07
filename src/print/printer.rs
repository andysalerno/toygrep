use super::{Config, PrintMessage, PrintableResult};
use crate::error::{Error, Result};
use crate::matcher::Matcher;
use std::collections::HashMap;
use std::io::Write;
use termcolor::{Color, ColorSpec, WriteColor};

/// This module contains the types and logic
/// for a printer that can group lines
/// and color matching patterns.
///
/// It is not exposed outside this module,
/// but module `threaded_printer` contains a
/// threaded wrapper, and module `blocking_printer`
/// contains a blocking wrapper that can be
/// used externally.

pub(super) struct PrettyPrinter<M: Matcher> {
    file_to_matches: HashMap<String, Vec<PrintableResult>>,
    currently_printing_file: Option<String>,
    config: Config,
    matcher: Option<M>,
}

impl<M: Matcher> PrettyPrinter<M> {
    pub(super) fn new(matcher: Option<M>, config: Config) -> Self {
        Self {
            matcher,
            config,
            file_to_matches: HashMap::new(),
            currently_printing_file: None,
        }
    }

    pub(super) fn print<W>(&mut self, mut writer: W, message: PrintMessage)
    where
        W: Write + WriteColor,
    {
        if self.config.group_by_target {
            match message {
                PrintMessage::Display(msg) => {
                    print!("{}", msg);
                }
                PrintMessage::Printable(printable) => {
                    if self.currently_printing_file == None {
                        self.currently_printing_file = Some(printable.target_name.clone());

                        // Print everything we've already stored for this file:
                        let _ = self.print_target_results(&mut writer, &printable.target_name);
                    }

                    if Some(&printable.target_name) == self.currently_printing_file.as_ref() {
                        let _ = self.print_line_result(&mut writer, printable);
                    } else {
                        let line_results = self
                            .file_to_matches
                            .entry(printable.target_name.to_owned())
                            .or_default();

                        line_results.push(printable);
                    }
                }
                PrintMessage::EndOfReading { target_name } => {
                    if Some(&target_name) == self.currently_printing_file.as_ref() {
                        self.currently_printing_file = None;
                    } else {
                        let _ = self.print_target_results(&mut writer, &target_name);
                    }
                }
            }
        } else if let PrintMessage::Printable(printable) = message {
            let _ = self.print_line_result(&mut writer, printable);
        }
    }

    fn print_target_results<W>(&mut self, writer: &mut W, name: &str) -> Result<()>
    where
        W: Write + WriteColor,
    {
        // TODO: continue on error and present results in end
        let matches_for_target = self.file_to_matches.remove(name).unwrap_or_default();

        if matches_for_target.is_empty() {
            // Nothing to do.
            return Ok(());
        }

        writeln!(writer, "\n{}", name).expect("Error writing to stdout.");
        for printable in matches_for_target {
            self.print_line_result(writer, printable)?;
        }

        Ok(())
    }

    fn print_line_result<W>(&self, writer: &mut W, printable: PrintableResult) -> Result<()>
    where
        W: Write + WriteColor,
    {
        let line_num = if self.config.print_line_num {
            format!("{}:", printable.line_num)
        } else {
            "".to_owned()
        };

        if let Some(matcher) = &self.matcher {
            Self::print_colorized(&line_num, matcher, writer, &printable);
        } else {
            write!(writer, "{}{}", line_num, printable.text_as_string()?)
                .expect("Error writing to stdout.");
        }

        Ok(())
    }

    fn print_colorized<W>(
        line_num_chunk: &str,
        matcher: &M,
        writer: &mut W,
        printable: &PrintableResult,
    ) where
        W: Write + WriteColor,
    {
        let text = &printable.text;

        let parse_utf8 = |bytes| {
            std::str::from_utf8(bytes)
                .map_err(|_| Error::Utf8PrintFail(printable.target_name.to_owned()))
        };

        // First, write the line num in green.
        writer
            .set_color(ColorSpec::new().set_fg(Some(Color::Green)))
            .expect("Failed setting color.");

        write!(writer, "{}", line_num_chunk).expect("Failed writing line num chunk.");

        // Then, reset color to print the non-matching segment.
        writer.reset().expect("Failed to reset stdout color.");

        let mut start = 0;
        for match_range in matcher.find_matches(text) {
            let until_match = &text[start..match_range.start];
            let during_match = &text[match_range.start..match_range.stop];

            if let Ok(text) = parse_utf8(until_match) {
                write!(writer, "{}", text).expect("Failure writing to stdout");
            } else {
                eprintln!("Utf8 parsing error for target: {}", printable.target_name);
            }

            // The match itself is printed in red.
            // stdout
            writer
                .set_color(ColorSpec::new().set_fg(Some(Color::Red)))
                .expect("Failed setting color.");

            if let Ok(text) = parse_utf8(during_match) {
                write!(writer, "{}", text).expect("Failure writing to stdout");
            } else {
                eprintln!("Utf8 parsing error for target: {}", printable.target_name);
            }

            writer.reset().expect("Failed to reset stdout color.");

            start = match_range.stop;
        }

        // print remainder after final match
        let remainder = &text[start..];

        if let Ok(text) = parse_utf8(remainder) {
            write!(writer, "{}", text).expect("Failure writing to stdout");
        } else {
            eprintln!("Utf8 parsing error for target: {}", printable.target_name);
        }
    }
}

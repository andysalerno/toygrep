//! Documentation in progress.

#![forbid(unsafe_code, rust_2018_idioms)]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    trivial_casts,
    trivial_numeric_casts
)]
#![warn(
    missing_docs,
    missing_doc_code_examples,
    unreachable_pub,
    future_incompatible
)]

mod arg_parse;
mod async_line_buffer;
mod printer;
mod search;
mod search_target;

use async_line_buffer::{AsyncLineBufferBuilder, AsyncLineBufferReader};
use async_std::io::BufReader;
use async_std::path::Path;
use printer::StdOutPrinter;
use regex::RegexBuilder;
use search_target::SearchTarget;
use std::sync::mpsc;
use std::thread;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let user_input = {
        let args = std::env::args();
        arg_parse::capture_input(args)
    };

    if user_input.debug_enabled {
        dbg!("Targets: {:?}", &user_input.search_targets);
    }

    let regex = {
        let with_whole_word = if user_input.whole_word {
            format_word_match(user_input.search_pattern)
        } else {
            user_input.search_pattern
        };

        RegexBuilder::new(&with_whole_word)
            .case_insensitive(user_input.case_insensitive)
            .build()
            .unwrap_or_else(|e| panic!("{:?}", e))
    };

    let (sender, receiver) = mpsc::channel();
    let printer = StdOutPrinter::new(receiver);
    let printer_handle = thread::spawn(move || {
        printer.listen();
    });

    if user_input.search_target == SearchTarget::Stdin {
        let file_rdr = BufReader::new(async_std::io::stdin());
        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(8000)
            .build();

        let line_rdr = AsyncLineBufferReader::new(file_rdr, line_buf);

        search::search_via_reader(&regex, line_rdr, sender.clone()).await;
    } else {
        for target in user_input.search_targets {
            let path: &Path = &target;
            search::search_target(path, &regex, sender.clone()).await;
        }
    };

    drop(sender);

    printer_handle
        .join()
        .expect("Failed to join the printer thread.");

    Ok(())
}

fn format_word_match(pattern: String) -> String {
    format!(r"(?:(?m:^)|\W)({})(?:(?m:$)|\W)", pattern)
}

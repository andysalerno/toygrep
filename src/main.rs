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
mod error;
mod matcher;
mod printer;
mod search;
mod search_target;

use async_line_buffer::{AsyncLineBufferBuilder, AsyncLineBufferReader};
use async_std::io::BufReader;
use async_std::path::Path;
use matcher::RegexMatcherBuilder;
use printer::StdOutPrinterBuilder;
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

    let matcher = RegexMatcherBuilder::new()
        .for_pattern(&user_input.search_pattern)
        .case_insensitive(user_input.case_insensitive)
        .match_whole_word(user_input.whole_word)
        .build();

    let (sender, receiver) = mpsc::channel();
    let mut printer = StdOutPrinterBuilder::new(receiver)
        .with_matcher(matcher.clone())
        .build();

    let printer_handle = thread::spawn(move || {
        printer.listen();
    });

    if user_input.search_target == SearchTarget::Stdin {
        let file_rdr = BufReader::new(async_std::io::stdin());
        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(8000)
            .build();

        let line_rdr = AsyncLineBufferReader::new(file_rdr, line_buf);

        search::search_via_reader(matcher, line_rdr, None, sender.clone()).await;
    } else {
        for target in user_input.search_targets {
            let path: &Path = &target;
            let matcher = matcher.clone();
            search::search_target(path, matcher, sender.clone()).await;
        }
    };

    drop(sender);

    printer_handle
        .join()
        .expect("Failed to join the printer thread.");

    Ok(())
}

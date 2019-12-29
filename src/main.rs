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
mod target;

use async_line_buffer::{AsyncLineBufferBuilder, AsyncLineBufferReader};
use async_std::io::BufReader;
use async_std::path::Path;
use matcher::RegexMatcherBuilder;
use printer::threaded_printer::{ThreadedPrinterBuilder, ThreadedPrinterSender};
use std::clone::Clone;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;
use target::SearchTarget;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let user_input = {
        let args = std::env::args();
        arg_parse::capture_input(args)
    };

    let now = if user_input.stats {
        Some(Instant::now())
    } else {
        None
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
    let mut printer = ThreadedPrinterBuilder::new(receiver)
        .with_matcher(matcher.clone())
        .group_by_target(user_input.search_target != SearchTarget::Stdin)
        .build();
    let printer_sender = ThreadedPrinterSender::new(sender);

    let printer_handle = thread::spawn(move || {
        printer.listen();
    });

    if user_input.search_target == SearchTarget::Stdin {
        let file_rdr = BufReader::new(async_std::io::stdin());
        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(16_000)
            .build();

        let line_rdr = AsyncLineBufferReader::new(file_rdr, line_buf).line_nums(false);

        search::search_via_reader(matcher, line_rdr, None, printer_sender.clone()).await;
    } else {
        for target in user_input.search_targets {
            let path: &Path = &target;
            let matcher = matcher.clone();
            search::search_target(path, matcher, printer_sender.clone()).await;
        }
    };

    let elapsed = now.map(|n| n.elapsed());

    drop(printer_sender);

    printer_handle
        .join()
        .expect("Failed to join the printer thread.");

    if let Some(elapsed) = elapsed {
        println!("Time to search (ms): {}", elapsed.as_millis());
    }

    Ok(())
}

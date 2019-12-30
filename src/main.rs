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

use matcher::RegexMatcherBuilder;
use printer::threaded_printer::{ThreadedPrinterBuilder, ThreadedPrinterSender};
use std::clone::Clone;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let user_input = arg_parse::capture_input(std::env::args());

    let now = if user_input.stats {
        Some(Instant::now())
    } else {
        None
    };

    let matcher = RegexMatcherBuilder::new()
        .for_pattern(&user_input.search_pattern)
        .case_insensitive(user_input.case_insensitive)
        .match_whole_word(user_input.whole_word)
        .build();

    let (printer_handle, printer_sender) = {
        let (sender, receiver) = mpsc::channel();

        let mut printer = ThreadedPrinterBuilder::new(receiver)
            .with_matcher(matcher.clone())
            .group_by_target(user_input.targets.len() > 1)
            .print_immediately(
                user_input.targets.len() == 1
                    && user_input.targets.first().unwrap().is_file().await,
            )
            .build();

        let printer_sender = ThreadedPrinterSender::new(sender);

        let printer_handle = thread::spawn(move || {
            printer.listen();
        });

        (printer_handle, printer_sender)
    };

    search::search_targets(&user_input.targets, matcher, printer_sender.clone()).await;

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

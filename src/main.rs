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

use crate::error::Error;
use crate::search::SearcherBuilder;
use matcher::RegexMatcherBuilder;
use printer::threaded_printer::{ThreadedPrinterBuilder, ThreadedPrinterSender};
use std::clone::Clone;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

#[async_std::main]
async fn main() {
    let user_input = arg_parse::capture_input(std::env::args());

    let now = if user_input.stats {
        Some(Instant::now())
    } else {
        None
    };

    if user_input.search_pattern.is_empty() {
        print_help();
        return;
    }

    let matcher = RegexMatcherBuilder::new()
        .for_pattern(&user_input.search_pattern)
        .case_insensitive(user_input.case_insensitive)
        .match_whole_word(user_input.whole_word)
        .build();

    let (printer_handle, printer_sender) = {
        let (sender, receiver) = mpsc::channel();

        let first_target = user_input.targets.first();

        let print_immediately =
            user_input.targets.len() == 1 && first_target.unwrap().is_file().await;

        let group_by_target = user_input.targets.len() > 1
            || (first_target.is_some() && first_target.unwrap().is_dir().await);

        let printer = ThreadedPrinterBuilder::new(receiver)
            .with_matcher(matcher.clone())
            .group_by_target(group_by_target)
            .print_immediately(print_immediately)
            .build();

        let printer_sender = ThreadedPrinterSender::new(sender);
        let printer_handle = printer.spawn();

        (printer_handle, printer_sender)
    };

    let status = {
        let searcher = SearcherBuilder::new(matcher, printer_sender).build();
        searcher.search(&user_input.targets).await
    };

    let elapsed = now.map(|n| n.elapsed());

    printer_handle
        .join()
        .expect("Failed to join the printer thread.");

    if let Err(Error::TargetsNotFound(targets)) = status {
        eprintln!("\nInvalid targets specified: {:?}", targets);
    }

    if let Some(elapsed) = elapsed {
        println!("Time to search (ms): {}", elapsed.as_millis());
        println!(
            "    Total time (ms): {}",
            now.unwrap().elapsed().as_millis()
        );
    }
}

fn print_help() {
    let exec_name: String = {
        let canonical = PathBuf::from(std::env::args().next().unwrap());
        let os_str = canonical.file_name().unwrap();
        os_str.to_string_lossy().into()
    };

    println!(
        "Usage:
{} [OPTION]... PATTERN [FILE]...
    Options:
    -i      Case insensitive match.
    -w      Match whole word.
    -d      Print debug info with output.
    -t      Print statistical information with output.",
        exec_name
    );
}

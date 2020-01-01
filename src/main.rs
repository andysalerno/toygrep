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
mod time_log;

use crate::error::Error;
use crate::search::stats::ReadStats;
use crate::search::SearcherBuilder;
use crate::time_log::TimeLog;
use matcher::RegexMatcherBuilder;
use printer::threaded_printer::{ThreadedPrinterBuilder, ThreadedPrinterSender};
use std::clone::Clone;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

#[async_std::main]
async fn main() {
    let user_input = arg_parse::capture_input(std::env::args());

    let mut time_log = TimeLog::new(Instant::now());

    if user_input.search_pattern.is_empty() {
        print_help();
        return;
    }

    let matcher = RegexMatcherBuilder::new()
        .for_pattern(&user_input.search_pattern)
        .case_insensitive(user_input.case_insensitive)
        .match_whole_word(user_input.whole_word)
        .build();

    // The printer is spawned on a separate thread, giving us a channel
    // sender that can be cloned across async searches to send it results.
    // (Note: separate THREAD, -not- an async task.)
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

    // Perform the search, walking the filesystem, detecting matches,
    // and sending them to the printer (note, even after `search` has
    // terminated, the printer thread is likely still processing
    // the results sent to it).
    let status = {
        let searcher = SearcherBuilder::new(matcher, printer_sender).build();
        searcher.search(&user_input.targets).await
    };

    time_log.log_search_duration();

    // The printer thread stays alive as long as any channel senders exist.
    // At this point, we've queued up all our searches, so now we must wait
    // for them to complete, send the results to the printer, and drop their
    // respective senders.
    let print_time_log = printer_handle
        .join()
        .expect("Failed to join the printer thread.");

    time_log.print_duration = print_time_log.print_duration;
    time_log.printer_spawn_to_print = print_time_log.printer_spawn_to_print;
    time_log.first_result_to_first_print = print_time_log.first_result_to_first_print;

    if let Err(Error::TargetsNotFound(targets)) = &status {
        eprintln!("\nInvalid targets specified: {:?}", targets);
    }

    time_log.log_start_die_duration();
    if user_input.stats && status.is_ok() {
        let stats = status.unwrap();
        println!("{}", format_stats(&stats, &time_log));
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
    -t      Print statistical information with output.",
        exec_name
    );
}

fn format_stats(read_stats: &ReadStats, time_log: &TimeLog) -> String {
    format!(
        "\n{} total files visited
{} skipped (non-utf8) files
{} total bytes checked for non-utf8 detection
{} matching lines found
{} total bytes in matching lines
{} seconds start-to-stop
{} seconds searching
{} seconds until first result arrives at printer 
{} seconds between first result arriving and first printing
{} seconds printing",
        read_stats.total_files_visited,
        read_stats.skipped_files_non_utf8,
        read_stats.non_utf8_bytes_checked,
        read_stats.lines_matched_count,
        read_stats.lines_matched_bytes,
        time_log
            .start_die_duration
            .map(|d| d.as_secs_f32().to_string())
            .unwrap_or_else(|| "(not measured)".into()),
        time_log
            .search_duration
            .map(|d| d.as_secs_f32().to_string())
            .unwrap_or_else(|| "(not measured)".into()),
        time_log
            .printer_spawn_to_print
            .map(|d| d.as_secs_f32().to_string())
            .unwrap_or_else(|| "(not measured)".into()),
        time_log
            .first_result_to_first_print
            .map(|d| d.as_secs_f32().to_string())
            .unwrap_or_else(|| "(not measured)".into()),
        time_log
            .print_duration
            .map(|d| d.as_secs_f32().to_string())
            .unwrap_or_else(|| "(not measured)".into()),
    )
}

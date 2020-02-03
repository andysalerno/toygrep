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
mod buffer;
mod error;
mod matcher;
mod printer;
mod search;
mod target;
mod time_log;

use crate::error::Error;
use crate::printer::Printer;
use crate::search::stats::ReadStats;
use crate::search::SearcherBuilder;
use crate::time_log::TimeLog;
use matcher::RegexMatcherBuilder;
use std::clone::Clone;
use std::path::PathBuf;
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

    let print_builder = {
        let first_target = user_input.targets.first();

        let print_immediately =
            user_input.targets.len() == 1 && first_target.unwrap().is_file().await;

        let group_by_target = user_input.targets.len() > 1
            || (first_target.is_some() && first_target.unwrap().is_dir().await);

        Printer::new()
            .with_matcher(matcher.clone())
            .group_by_target(group_by_target)
            .print_immediately(print_immediately)
    };

    // Perform the search, walking the filesystem, detecting matches,
    // and sending them to the printer (note, even after `search` has
    // terminated, the printer thread is likely still processing
    // the results sent to it).
    let status = {
        // TODO: consider using dyn instead of branching
        if user_input.synchronous_printer {
            let printer = print_builder.build_blocking();
            let searcher = SearcherBuilder::new(matcher, printer).build();
            searcher.search(&user_input.targets).await
        } else {
            let (printer, join_handle) = print_builder.spawn_threaded();
            let searcher = SearcherBuilder::new(matcher, printer).build();
            let result = searcher.search(&user_input.targets).await;

            drop(searcher);

            join_handle.join().expect("Couldn't join printing thread.");

            result
        }
    };

    time_log.log_search_duration();

    // The printer thread stays alive as long as any channel senders exist.
    // At this point, we've queued up all our searches, so now we must wait
    // for them to complete, send the results to the printer, and drop their
    // respective senders.
    // let print_time_log = printer_handle
    //     .join()
    //     .expect("Failed to join the printer thread.");
    // time_log.print_duration = print_time_log.print_duration;
    // time_log.printer_spawn_to_print = print_time_log.printer_spawn_to_print;
    // time_log.first_result_to_first_print = print_time_log.first_result_to_first_print;

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
    -i, --case-insensitive      Case insensitive match.
    -w, --whole-word            Match whole word.
    -t, --stats                 Print statistical information with output.",
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
{max_buf_size} maximum buffer size (bytes)
{buffers_created} buffers created
{startstop} seconds start-to-stop
{filesystem} seconds recursing through filesystem
{search} seconds searching
{printidle} seconds until first result arrives at printer 
{printprint} seconds between first result arriving and first printing
{printing} seconds printing",
        read_stats.total_files_visited,
        read_stats.skipped_files_non_utf8,
        read_stats.non_utf8_bytes_checked,
        read_stats.lines_matched_count,
        read_stats.lines_matched_bytes,
        startstop = time_log
            .start_die_duration
            .map(|d| d.as_secs_f32().to_string())
            .unwrap_or_else(|| "(not measured)".into()),
        filesystem = read_stats.filesystem_walk_dur.as_secs_f32(),
        search = time_log
            .search_duration
            .map(|d| d.as_secs_f32().to_string())
            .unwrap_or_else(|| "(not measured)".into()),
        printidle = time_log
            .printer_spawn_to_print
            .map(|d| d.as_secs_f32().to_string())
            .unwrap_or_else(|| "(not measured)".into()),
        printprint = time_log
            .first_result_to_first_print
            .map(|d| d.as_secs_f32().to_string())
            .unwrap_or_else(|| "(not measured)".into()),
        printing = time_log
            .print_duration
            .map(|d| d.as_secs_f32().to_string())
            .unwrap_or_else(|| "(not measured)".into()),
        max_buf_size = read_stats.max_buffer_size,
        buffers_created = read_stats.buffers_created,
    )
}

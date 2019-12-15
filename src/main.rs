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
mod search_target;

use async_line_buffer::{AsyncLineBufferBuilder, AsyncLineBufferReader};
use async_std::fs::{self, File};
use async_std::io::Result as IoResult;
use async_std::io::{BufReader, Read};
use async_std::path::Path;
use async_std::prelude::*;
use regex::Regex;
use search_target::SearchTarget;
use std::str;
use std::sync::mpsc::channel;

#[async_std::main]
async fn main() -> IoResult<()> {
    let user_input = {
        let args = std::env::args();
        arg_parse::capture_input(args)
    };

    if user_input.debug_enabled {
        dbg!(&user_input);
    }

    if user_input.debug_enabled {
        dbg!("Targets: {:?}", &user_input.search_targets);
    }

    let pattern = {
        let case_insensitive_match = if user_input.case_insensitive {
            "(?i)"
        } else {
            ""
        };

        let whole_word_match = if user_input.whole_word { "\\b" } else { "" };

        format!(
            "{}{}{}",
            whole_word_match, case_insensitive_match, user_input.search_pattern
        )
    };

    let regex = Regex::new(&pattern)
        .unwrap_or_else(|_| panic!("Invalid search expression: {}", &user_input.search_pattern));

    if let SearchTarget::Stdin = user_input.search_target {
        let file_rdr = BufReader::new(async_std::io::stdin());
        let line_buf = AsyncLineBufferBuilder::new()
            .with_minimum_read_size(8000)
            .build();

        let line_rdr = AsyncLineBufferReader::new(file_rdr, line_buf);

        let search_result = search_via_reader(&regex, line_rdr).await;
        println!("{}", search_result);
    } else {
        for target in user_input.search_targets {
            let path: &Path = &target;

            let search_result = search_target(path, &regex).await?;

            println!("{}", search_result);
        }
    }

    Ok(())
}

async fn search_target(target_path: impl Into<&Path>, pattern: &Regex) -> IoResult<String> {
    // If the target is a file, search it.
    let target_path = target_path.into();
    if target_path.is_file().await {
        search_file(target_path, pattern).await
    } else if target_path.is_dir().await {
        // If it's a directory, recurse into it and search all its contents.
        search_directory(target_path, pattern).await
    } else {
        panic!(
            "Couldn't find file or dir at path: {}. Btw, this should be an Err, not a panic...",
            target_path.display()
        );
    }
}

async fn search_directory(directory_path: &Path, pattern: &Regex) -> IoResult<String> {
    let (sender, receiver) = channel();

    sender
        .send(directory_path.to_path_buf())
        .expect("Failure establishing sync channel.");

    let mut spawned_tasks = Vec::new();

    for dir_path in receiver.try_iter() {
        let mut dir_children = fs::read_dir(dir_path).await?;

        while let Some(dir_child) = dir_children.next().await {
            let dir_child = dir_child?.path();
            let pattern = pattern.clone();

            if dir_child.is_file().await {
                let task = async_std::task::spawn(async move {
                    let dir_child_path: &Path = &dir_child;

                    search_file(dir_child_path, &pattern)
                        .await
                        .expect("search failed")
                });

                spawned_tasks.push(task);
            } else if dir_child.is_dir().await {
                sender
                    .send(dir_child)
                    .expect("Failure sending over sync channel.");
            }
        }
    }

    let mut search_result = String::new();

    for task in spawned_tasks {
        let mut result = task.await;
        search_result.extend(result.drain(..));
    }

    Ok(search_result)
}

async fn search_file(file_path: impl Into<&Path>, pattern: &Regex) -> IoResult<String> {
    let file = File::open(file_path.into()).await?;
    let rdr = BufReader::new(file);

    // TODO: use min-read-len of the filesize if filesize is relatively low
    let line_buf = AsyncLineBufferBuilder::new()
        .with_minimum_read_size(8000)
        .build();
    let line_buf_rdr = AsyncLineBufferReader::new(rdr, line_buf);

    let result = search_via_reader(pattern, line_buf_rdr).await;

    Ok(result)
}

async fn search_via_reader<R>(pattern: &Regex, mut buffer: AsyncLineBufferReader<R>) -> String
where
    R: Read + std::marker::Unpin,
{
    // TODO: fiddle with capacity
    let mut result = String::new();

    while let Some(line_bytes) = buffer.read_line().await {
        let as_utf = str::from_utf8(&line_bytes).expect("Unable to parse line as utf8.");
        if pattern.is_match(as_utf) {
            result.push_str(as_utf);
        }
    }

    result
}

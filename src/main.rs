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
mod search_target;

use async_std::fs;
use async_std::io::Result as IoResult;
use async_std::io::{stdin, BufReader, Read};
use async_std::path::{Path, PathBuf};
use async_std::prelude::*;
use regex::Regex;
use search_target::SearchTarget;
use std::sync::mpsc::channel;

const BIG_FILE_PAR_SEARCH_LIMIT_BYTES: u64 = 10_000_000;

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
        let reader = BufReader::new(async_std::io::stdin());
        search_it(reader, &regex).await;
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

async fn search_it<R>(mut reader: R, pattern: &Regex)
where
    R: Read + std::marker::Unpin,
{
    // // Always the last line read, probably only the first fragment of it.
    let mut hanging_line = String::new();
    let mut buf = vec![0u8; 1024];

    while let Ok(r) = reader.read(&mut buf).await {
        if r == 0 {
            return;
        }

        let drained = buf;
        buf = vec![0u8; 1024];
        let as_str = String::from_utf8_lossy(&drained[..r]);

        let lines_in_chunk = as_str.lines().collect::<Vec<_>>();

        if lines_in_chunk.len() == 1 {
            // only one line indicates we didn't hit a newline yet
            hanging_line.push_str(lines_in_chunk[0]);
        } else if lines_in_chunk.len() > 1 {
            // There are multiple lines, so the first line + hanging = a complete line
            // This is a full line now:
            hanging_line.push_str(lines_in_chunk[0]);

            
        }

        let first_line = lines_in_chunk.next();

        println!("{}", &as_str);
    }
}

// fn read_smarter() {
//     let mut byte_stream = reader.bytes();
//     let mut current_line = String::new();

//     while let Some(b) = byte_stream.next().await {
//         match b {
//             '\n' => {}
//             _ => {}
//         }
//     }
// }

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
    let file_path = file_path.into();
    let file_size_bytes = file_size_bytes(file_path).await?;

    let content = fs::read_to_string(file_path).await?;

    // TODO: implement buffered reading to minimize memory
    if file_size_bytes > BIG_FILE_PAR_SEARCH_LIMIT_BYTES {
        // TODO: split file further
        search_chunk(&content, pattern).await
    } else {
        // Search the whole file
        search_chunk(&content, pattern).await
    }
}

async fn search_chunk(chunk: &str, pattern: &Regex) -> IoResult<String> {
    let mut result = String::new();

    for line in chunk.lines() {
        if pattern.is_match(line) {
            result.push_str(line);
            result.push('\n');
        }
    }

    Ok(result)
}

async fn file_size_bytes(file_path: &Path) -> IoResult<u64> {
    let metadata = fs::metadata(file_path).await?;

    Ok(metadata.len())
}

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

use async_std::fs;
use async_std::io::Result as IoResult;
use async_std::path::{Path, PathBuf};
use async_std::prelude::*;
use regex::Regex;
use std::sync::mpsc::channel;
use std::sync::Arc;

const BIG_FILE_PAR_SEARCH_LIMIT_BYTES: u64 = 10_000_000;

#[async_std::main]
async fn main() -> IoResult<()> {
    let args = std::env::args();

    let user_input = arg_parse::capture_input(args);

    if user_input.debug_enabled {
        dbg!(&user_input);
    }

    // TODO: lots of unnecessary copying happening below... there's certainly a better way.
    let targets = if user_input.search_targets.is_empty() {
        // By default, if no target is provided, search recursively from the current dir.
        let cur_dir_canonical = std::env::current_dir()?.canonicalize()?;

        // TODO: might only need to do "." or "./" for current path, and Regex takes care of it
        vec![cur_dir_canonical.to_owned()]
    } else {
        user_input.search_targets.clone()
    };

    if user_input.debug_enabled {
        dbg!("Targets: {:?}", &targets);
    }

    let regex = Regex::new(&user_input.search_pattern)
        .unwrap_or_else(|_| panic!("Invalid search expression: {}", &user_input.search_pattern));

    for target in targets {
        let path_buf: PathBuf = target.into();
        let path: &Path = &path_buf;
        let search_result = search_target(path, &regex).await?;

        println!("{}", search_result);
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
                    let child_search_result = search_file(dir_child_path, &pattern)
                        .await
                        .expect("search failed");

                    child_search_result
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

    if file_size_bytes > BIG_FILE_PAR_SEARCH_LIMIT_BYTES {
        search_chunk(content.lines(), pattern).await
    } else {
        let content = Arc::new(content);

        Ok("hello hello".to_string())
    }
}

// async fn search_chunk(chunk: &[&str], pattern: &Regex) -> IoResult<String> {
async fn search_chunk(chunk: impl Iterator<Item = &str>, pattern: &Regex) -> IoResult<String> {
    let mut result = String::new();

    for line in chunk {
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

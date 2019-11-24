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
use regex::Regex;
use std::path::Path;
use std::sync::mpsc::channel;

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
        let search_result = search_target(&target, &regex).await?;

        println!("{}", search_result);
    }

    Ok(())
}

async fn search_target(target_path: &Path, pattern: &Regex) -> IoResult<String> {
    // If the target is a file, search it.
    if target_path.is_file() {
        search_file(target_path, pattern).await
    } else if target_path.is_dir() {
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
        let dir_children = dir_path.read_dir()?;

        for dir_child in dir_children {
            let dir_child = dir_child?.path();
            let pattern = pattern.clone();

            if dir_child.is_file() {
                let task = async_std::task::spawn(async move {
                    let pattern = pattern.clone();
                    let child_search_result = search_file(&dir_child, &pattern)
                        .await
                        .expect("search failed");

                    child_search_result
                });

                spawned_tasks.push(task);
            } else if dir_child.is_dir() {
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

async fn search_file(file_path: &Path, pattern: &Regex) -> IoResult<String> {
    let bytes = fs::read(file_path).await?;

    let sample = &bytes[17960..17970];
    println!("{:?}", sample);

    // let test = std::str::from_utf8(&bytes);

    // if let Err(e) = test {
    //     panic!("problem: {:?}", e);
    // }

    let content = fs::read_to_string(file_path).await?;

    // let as_utf8 = String::from_utf8(content);

    let mut result = String::new();

    let lines = content.lines();

    for line in lines {
        if pattern.is_match(line) {
            result.push_str(line);
            result.push('\n');
        }
    }

    Ok(result)
}

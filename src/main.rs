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
        panic!(
            "Directories not supported yet. Directory: {}",
            target_path.display()
        );
    } else {
        panic!(
            "Couldn't find file or dir at path: {}. Btw, this should be an Err, not a panic...",
            target_path.display()
        );
    }
}

async fn search_file(file_path: &Path, pattern: &Regex) -> IoResult<String> {
    let content = fs::read_to_string(file_path).await?;
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

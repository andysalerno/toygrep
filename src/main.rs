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

#[async_std::main]
async fn main() -> IoResult<()> {
    let args = std::env::args();

    let user_input = arg_parse::capture_input(args);

    if user_input.debug_enabled {
        dbg!(&user_input);
    }

    let regex = Regex::new(&user_input.search_pattern)
        .unwrap_or_else(|_| panic!("Invalid search expression: {}", &user_input.search_pattern));

    let search_result = search_file(&user_input.search_targets[0], &regex).await?;

    println!("{}", search_result);

    Ok(())
}

async fn search_file(file_path: &str, pattern: &Regex) -> IoResult<String> {
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

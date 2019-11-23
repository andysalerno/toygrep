mod arg_parse;

use async_std::fs;
use async_std::io::Result as IoResult;
use regex::Regex;

#[async_std::main]
async fn main() -> IoResult<()> {
    let args = std::env::args();

    let user_input = arg_parse::capture_input(args);

    dbg!(&user_input);

    let regex = Regex::new(&user_input.search_pattern).expect(&format!(
        "Invalid search expression: {}",
        &user_input.search_pattern
    ));

    search_file(&user_input.search_targets[0], &regex).await?;

    Ok(())
}

async fn search_file(file_path: &str, pattern: &Regex) -> IoResult<()> {
    let content = fs::read_to_string(file_path).await?;

    let lines = content.lines();

    for line in lines {
        if pattern.is_match(line) {
            println!("Found match: {}", line);
        }
    }

    Ok(())
}

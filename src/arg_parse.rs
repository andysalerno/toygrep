use peeking_take_while::PeekableExt;

#[derive(Debug, Default)]
pub struct UserInput {
    search_pattern: String,
    search_targets: Vec<String>,

    recursive: bool,
    whole_word: bool,
    case_insensitive: bool,
}

/// Parses the given arguments, following this expected format:
/// toygrep [OPTION]... PATTERN [FILE]...
pub fn capture_input(args: impl Iterator<Item = String>) -> UserInput {
    let mut user_input = UserInput::default();

    let mut args = args.skip(1).peekable();

    // Flags come first.
    for arg in args.by_ref().peeking_take_while(|a| a.starts_with("-")) {
        // TODO: support combined flags, like '-iwr'
        match arg.as_str() {
            "-i" => user_input.case_insensitive = true,
            "-w" => user_input.whole_word = true,
            "-r" => user_input.recursive = true,
            _ => {
                panic!("Unknown flag: {}", arg);
            }
        }
    }

    // The search pattern is next.
    if let Some(pattern) = args.next() {
        user_input.search_pattern = pattern;
    }

    // Finally, the file(s)/directory(ies) to search.
    user_input.search_targets = args.collect();

    user_input
}

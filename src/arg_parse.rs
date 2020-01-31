use crate::target::Target;
use peeking_take_while::PeekableExt;

#[derive(Debug, Default)]
pub(crate) struct UserInput {
    pub(crate) search_pattern: String,

    pub(crate) whole_word: bool,
    pub(crate) case_insensitive: bool,
    pub(crate) synchronous_printer: bool,

    pub(crate) targets: Vec<Target>,

    pub(crate) stats: bool,
}

/// Parses the given arguments, following this expected format:
/// toygrep [OPTION]... PATTERN [FILE]...
pub(crate) fn capture_input(args: impl Iterator<Item = String>) -> UserInput {
    let mut user_input = UserInput::default();

    // Skip the first arg (executable name).
    let mut args = args.skip(1).peekable();

    // Flags come first.
    for arg in args.by_ref().peeking_take_while(|a| a.starts_with('-')) {
        // TODO: support combined flags, like '-iwr'
        match arg.as_str() {
            "-i" | "--case-insensitive" => user_input.case_insensitive = true,
            "-w" | "--whole-word" => user_input.whole_word = true,
            "-t" | "--stats" => user_input.stats = true,
            "-p" | "--sync-print" => user_input.synchronous_printer = true,
            _ => {
                panic!("Unknown flag: {}", arg);
            }
        }
    }

    // The search pattern is next.
    if let Some(pattern) = args.next() {
        user_input.search_pattern = pattern;
    }

    user_input.targets = if is_stdin_provided() {
        vec![Target::Stdin]
    } else {
        args.map(|a| a.into()).map(Target::for_path).collect()
    };

    if user_input.targets.is_empty() {
        let current_dir = std::env::current_dir().expect("Unable to access the current directory.");
        user_input.targets = vec![Target::for_path(current_dir.into())];
    }

    user_input
}

fn is_stdin_provided() -> bool {
    atty::isnt(atty::Stream::Stdin)
}

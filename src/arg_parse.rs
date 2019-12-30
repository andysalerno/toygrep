use crate::target::Target;
use peeking_take_while::PeekableExt;

#[derive(Debug, Default)]
pub(crate) struct UserInput {
    pub(crate) debug_enabled: bool,

    pub(crate) search_pattern: String,

    pub(crate) whole_word: bool,
    pub(crate) case_insensitive: bool,

    pub(crate) targets: Vec<Target>,

    pub(crate) stats: bool,
}

/// Parses the given arguments, following this expected format:
/// toygrep [OPTION]... PATTERN [FILE]...
pub(crate) fn capture_input(args: impl Iterator<Item = String>) -> UserInput {
    let mut user_input = UserInput::default();

    let mut args = args.skip(1).peekable();

    // Flags come first.
    for arg in args.by_ref().peeking_take_while(|a| a.starts_with('-')) {
        // TODO: support combined flags, like '-iwr'
        match arg.as_str() {
            "-i" => user_input.case_insensitive = true,
            "-w" => user_input.whole_word = true,
            "-d" => user_input.debug_enabled = true,
            "-t" => user_input.stats = true,
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

    user_input
}

fn is_stdin_provided() -> bool {
    atty::isnt(atty::Stream::Stdin)
}

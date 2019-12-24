use regex::bytes::{Regex, RegexBuilder};

/// A trait that promises to answer a simple question:
/// does the given slice of bytes match a specific pattern?
pub(crate) trait Matcher: Clone + Send + Sync + Sized {
    fn is_match(&self, bytes: &[u8]) -> bool;
}

#[derive(Debug, Clone)]
pub(crate) struct RegexMatcher {
    regex: Regex,
}

impl Matcher for RegexMatcher {
    fn is_match(&self, bytes: &[u8]) -> bool {
        self.regex.is_match(bytes)
    }
}

pub(crate) struct RegexMatcherBuilder<'a> {
    pattern: &'a str,
    is_case_insensitive: bool,
    match_whole_word: bool,
}

impl<'a> RegexMatcherBuilder<'a> {
    pub(crate) fn new() -> Self {
        Self {
            is_case_insensitive: true,
            match_whole_word: false,
            pattern: "",
        }
    }

    pub(crate) fn for_pattern(mut self, pattern: &'a str) -> Self {
        self.pattern = pattern;
        self
    }

    pub(crate) fn case_insensitive(mut self, is_case_insensitive: bool) -> Self {
        self.is_case_insensitive = is_case_insensitive;
        self
    }

    pub(crate) fn match_whole_word(mut self, match_whole_word: bool) -> Self {
        self.match_whole_word = match_whole_word;
        self
    }

    pub(crate) fn build(self) -> RegexMatcher {
        let regex = {
            let with_whole_word = if self.match_whole_word {
                format_word_match(self.pattern)
            } else {
                self.pattern.to_owned()
            };

            RegexBuilder::new(&with_whole_word)
                .case_insensitive(self.is_case_insensitive)
                .build()
                .unwrap_or_else(|e| panic!("{:?}", e))
        };

        RegexMatcher { regex }
    }
}

fn format_word_match(pattern: &str) -> String {
    format!(r"(?:(?m:^)|\W)({})(?:(?m:$)|\W)", pattern)
}

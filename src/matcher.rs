use regex::bytes::{Regex, RegexBuilder};

#[derive(Debug, Clone)]
pub(crate) struct Match {
    pub(crate) start: usize,
    pub(crate) stop: usize,
}

/// A trait that promises to answer a simple question:
/// does the given slice of bytes match a specific pattern?
pub(crate) trait Matcher: Clone + Send {
    fn is_match(&self, bytes: &[u8]) -> bool;
    fn find_matches(&self, bytes: &[u8]) -> Vec<Match>;
}

/// A stub of a Matcher that never finds a match.
#[derive(Debug, Clone)]
pub(crate) struct DummyMatcher;

impl Matcher for DummyMatcher {
    fn is_match(&self, _bytes: &[u8]) -> bool {
        false
    }
    fn find_matches(&self, _bytes: &[u8]) -> Vec<Match> {
        Vec::new()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RegexMatcher {
    regex: Regex,
}

impl Matcher for RegexMatcher {
    fn is_match(&self, bytes: &[u8]) -> bool {
        self.regex.is_match(bytes)
    }

    fn find_matches(&self, bytes: &[u8]) -> Vec<Match> {
        self.regex
            .find_iter(bytes)
            .map(|m| Match {
                start: m.start(),
                stop: m.end(),
            })
            .collect()
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

use regex::bytes::Regex;

pub(crate) trait Matcher: Clone + Send + Sync {
    fn is_match(&self, bytes: &[u8]) -> bool;
}

#[derive(Debug, Clone)]
pub(crate) struct RegexMatcher {
    regex: Regex,
}

impl RegexMatcher {
    pub(crate) fn new(regex: Regex) -> Self {
        Self { regex }
    }
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
    pub fn new() -> Self {
        Self {
            is_case_insensitive: true,
            match_whole_word: false,
            pattern: "",
        }
    }

    pub fn for_pattern(self, pattern: &'a str) -> Self {
        self.pattern = pattern;
        self
    }

    pub fn case_insensitive(self, is_case_insensitive: bool) -> Self {
        self.is_case_insensitive = is_case_insensitive;
        self
    }

    pub fn match_whole_word(self, match_whole_word: bool) -> Self {
        self.match_whole_word = match_whole_word;
        self
    }

    pub fn build(self) -> RegexMatcher {
        todo!()
        // let regex = {
        //     let with_whole_word = if user_input.whole_word {
        //         format_word_match(user_input.search_pattern)
        //     } else {
        //         user_input.search_pattern
        //     };

        //     RegexBuilder::new(&with_whole_word)
        //         .case_insensitive(user_input.case_insensitive)
        //         .build()
        //         .unwrap_or_else(|e| panic!("{:?}", e))
        // };
    }
}

fn format_word_match(pattern: String) -> String {
    format!(r"(?:(?m:^)|\W)({})(?:(?m:$)|\W)", pattern)
}

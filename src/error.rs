pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub(crate) enum Error {
    Utf8PrintFail(String),
    TargetsNotFound(Vec<String>),
}

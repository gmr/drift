use std::path::PathBuf;

/// Anything that can stop an analysis.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot open a git repository at {path}")]
    OpenRepository {
        path: PathBuf,
        #[source]
        source: Box<gix::discover::Error>,
    },

    #[error("cannot resolve `{rev}` to a single commit")]
    ResolveRev {
        rev: String,
        #[source]
        source: BoxedError,
    },

    #[error("cannot read git object data")]
    Git(#[source] BoxedError),

    #[error("{path} line {line}: invalid gitignore pattern")]
    IgnorePattern {
        path: String,
        line: usize,
        #[source]
        source: Box<ignore::Error>,
    },

    #[error("{path}: cannot build the ignore matcher")]
    IgnoreBuild {
        path: String,
        #[source]
        source: Box<ignore::Error>,
    },

    #[error("{path} is not valid UTF-8")]
    IgnoreEncoding { path: String },

    #[error("the path {path} is not valid UTF-8")]
    PathEncoding { path: String },

    #[error("`{from}` and `{to}` share no common ancestor")]
    MergeBase {
        from: String,
        to: String,
        #[source]
        source: BoxedError,
    },

    #[error("commit {sha} has a timestamp outside the supported range")]
    CommitTime {
        sha: String,
        #[source]
        source: BoxedError,
    },

    #[error("cannot serialize the report")]
    Json(#[from] serde_json::Error),

    #[error("cannot write the report to stdout")]
    Output(#[source] std::io::Error),
}

pub type BoxedError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub type Result<T> = std::result::Result<T, Error>;

/// Wrap any gix error as [`Error::Git`].
///
/// The gix error types are deep and vary per call site. The message they carry is
/// what a user needs, so keep the source chain and drop the static typing.
pub(crate) fn git<E>(source: E) -> Error
where
    E: std::error::Error + Send + Sync + 'static,
{
    Error::Git(Box::new(source))
}

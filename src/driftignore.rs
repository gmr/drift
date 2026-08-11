use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::error::{Error, Result};

/// The path of the ignore file inside the tree of the newer ref.
pub const FILE_NAME: &str = ".driftignore";

/// Gitignore style patterns that mark paths as expected to change.
///
/// The patterns come from the newer ref's tree, not from the worktree, so two runs
/// over the same commit range always classify commits the same way.
#[derive(Debug)]
pub struct DriftIgnore {
    matcher: Gitignore,
    present: bool,
}

impl DriftIgnore {
    /// Read `.driftignore` from the tree of `commit`.
    ///
    /// A missing file yields an empty rule set, which makes every changed path drift.
    pub fn from_commit(commit: &gix::Commit<'_>) -> Result<Self> {
        let tree = commit.tree().map_err(crate::error::git)?;
        let Some(entry) = tree
            .lookup_entry_by_path(FILE_NAME)
            .map_err(crate::error::git)?
        else {
            return Ok(Self {
                matcher: Gitignore::empty(),
                present: false,
            });
        };
        let object = entry.object().map_err(crate::error::git)?;
        let contents = std::str::from_utf8(&object.data).map_err(|_| Error::IgnoreEncoding {
            path: FILE_NAME.to_string(),
        })?;
        Self::parse(contents)
    }

    /// Build a matcher from the contents of a `.driftignore` file.
    pub fn parse(contents: &str) -> Result<Self> {
        // The repository root is the anchor for every pattern, so relative paths taken
        // from a tree diff can be matched directly.
        let mut builder = GitignoreBuilder::new("");
        for (index, line) in contents.lines().enumerate() {
            builder
                .add_line(None, line)
                .map_err(|source| Error::IgnorePattern {
                    path: FILE_NAME.to_string(),
                    line: index + 1,
                    source: Box::new(source),
                })?;
        }
        Ok(Self {
            matcher: builder.build().map_err(|source| Error::IgnoreBuild {
                path: FILE_NAME.to_string(),
                source: Box::new(source),
            })?,
            present: true,
        })
    }

    /// Whether the newer ref carries a `.driftignore` file.
    pub fn is_present(&self) -> bool {
        self.present
    }

    /// Whether `path`, relative to the repository root, is expected to change.
    ///
    /// A negated pattern (`!keep-me`) wins over an earlier match, and it re-includes a
    /// path even when an earlier pattern excluded a parent directory. Git ignores a
    /// negation in that position; the README records the difference as deliberate.
    pub fn is_ignored(&self, path: &str) -> bool {
        self.matcher
            .matched_path_or_any_parents(path, false)
            .is_ignore()
    }
}

#[cfg(test)]
mod tests {
    use super::DriftIgnore;

    fn matcher(contents: &str) -> DriftIgnore {
        DriftIgnore::parse(contents).expect("valid patterns")
    }

    #[test]
    fn empty_ignores_nothing() {
        assert!(!matcher("").is_ignored("src/main.rs"));
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let ignore = matcher("# a comment\n\nCHANGELOG.md\n");
        assert!(ignore.is_ignored("CHANGELOG.md"));
        assert!(!ignore.is_ignored("src/main.rs"));
    }

    #[test]
    fn directory_pattern_covers_children() {
        let ignore = matcher("docs/\n");
        assert!(ignore.is_ignored("docs/guide/index.md"));
        assert!(!ignore.is_ignored("src/docs.rs"));
    }

    #[test]
    fn negation_reinstates_a_path() {
        let ignore = matcher("docs/\n!docs/api.md\n");
        assert!(ignore.is_ignored("docs/guide.md"));
        assert!(!ignore.is_ignored("docs/api.md"));
    }

    #[test]
    fn anchored_pattern_matches_only_at_the_root() {
        let ignore = matcher("/Cargo.lock\n");
        assert!(ignore.is_ignored("Cargo.lock"));
        assert!(!ignore.is_ignored("vendor/thing/Cargo.lock"));
    }

    #[test]
    fn invalid_pattern_reports_its_line() {
        let error = DriftIgnore::parse("ok.txt\n{unclosed\n").expect_err("invalid");
        assert!(error.to_string().contains("line 2"), "{error}");
    }
}

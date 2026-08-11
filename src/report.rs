use jiff::Timestamp;
use serde::Serialize;

use crate::mode::Mode;

/// A commit that changed at least one drifting path.
#[derive(Debug, Clone)]
pub struct DriftCommit {
    pub sha: String,
    /// `Name <email>` taken from the commit author, not the committer.
    pub author: String,
    /// Committer timestamp, which is what git orders history by.
    pub committed_at: Timestamp,
    /// The drifting paths this commit touched, sorted, relative to the repository root.
    pub paths: Vec<String>,
}

/// One end of the drift window.
#[derive(Debug, Clone, Serialize)]
pub struct DriftBound {
    pub sha: String,
    pub committed_at: String,
    pub age_seconds: i64,
}

impl DriftBound {
    fn new(commit: &DriftCommit, now: Timestamp) -> Self {
        Self {
            sha: commit.sha.clone(),
            committed_at: commit.committed_at.to_string(),
            age_seconds: now.as_second() - commit.committed_at.as_second(),
        }
    }
}

/// The paths behind one drift commit.
#[derive(Debug, Clone, Serialize)]
pub struct DriftDetail {
    pub sha: String,
    pub author: String,
    pub committed_at: String,
    pub paths: Vec<String>,
}

/// Everything an analysis found, before it is shaped into a report.
pub(crate) struct Summary {
    pub from: String,
    pub to: String,
    pub mode: Mode,
    pub driftignore_present: bool,
    pub commits_scanned: usize,
    pub merge_commits_skipped: usize,
    pub diverged: bool,
    pub commits: Vec<DriftCommit>,
    pub unattributed_paths: Vec<String>,
}

/// The machine readable result of one analysis.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub drift_detected: bool,
    /// Which set of paths was considered: `log` or `tree`.
    pub mode: Mode,
    /// The resolved sha of the older ref, which is excluded from the commit range.
    pub from: String,
    /// The resolved sha of the newer ref, which is included, and which supplies
    /// `.driftignore`.
    pub to: String,
    /// Whether the newer ref carries a `.driftignore` file.
    pub driftignore_present: bool,
    /// Whether the refs have each moved since they last shared a commit. When true,
    /// `from` holds work that `to` does not, so the comparison is not a clean promotion.
    pub diverged: bool,
    /// Non-merge commits examined in the range.
    pub commits_scanned: usize,
    /// Merge commits traversed but not classified.
    pub merge_commits_skipped: usize,
    pub drift_commit_count: usize,
    /// Drift commit shas, newest first.
    pub drift_commits: Vec<String>,
    /// Distinct `Name <email>` authors of the drift commits, sorted.
    pub drift_authors: Vec<String>,
    /// Every drifting path, sorted.
    pub drift_paths: Vec<String>,
    /// Drifting paths that no commit in the range explains, sorted. Only `tree` mode
    /// can produce these, and only when the refs have diverged: the difference comes
    /// from the `from` side.
    pub unattributed_paths: Vec<String>,
    pub oldest_drift: Option<DriftBound>,
    pub newest_drift: Option<DriftBound>,
    /// Every drift commit with the paths that caused it, newest first.
    pub details: Vec<DriftDetail>,
}

impl Report {
    /// Shape an analysis into its reported form.
    ///
    /// `now` is the single clock reading behind both ages, so the two ends of the
    /// window are always consistent with each other.
    pub(crate) fn new(summary: Summary, now: Timestamp) -> Self {
        let Summary {
            from,
            to,
            mode,
            driftignore_present,
            commits_scanned,
            merge_commits_skipped,
            diverged,
            mut commits,
            unattributed_paths,
        } = summary;

        commits.sort_by(|a, b| {
            b.committed_at
                .cmp(&a.committed_at)
                .then_with(|| a.sha.cmp(&b.sha))
        });

        let mut authors: Vec<String> = commits.iter().map(|c| c.author.clone()).collect();
        authors.sort();
        authors.dedup();

        let mut paths: Vec<String> = commits
            .iter()
            .flat_map(|c| c.paths.iter().cloned())
            .chain(unattributed_paths.iter().cloned())
            .collect();
        paths.sort();
        paths.dedup();

        Self {
            // An unattributed path is still drift: the refs differ because of it.
            drift_detected: !commits.is_empty() || !unattributed_paths.is_empty(),
            mode,
            from,
            to,
            driftignore_present,
            diverged,
            commits_scanned,
            merge_commits_skipped,
            drift_commit_count: commits.len(),
            drift_commits: commits.iter().map(|c| c.sha.clone()).collect(),
            drift_authors: authors,
            drift_paths: paths,
            unattributed_paths,
            oldest_drift: commits.last().map(|c| DriftBound::new(c, now)),
            newest_drift: commits.first().map(|c| DriftBound::new(c, now)),
            details: commits
                .iter()
                .map(|c| DriftDetail {
                    sha: c.sha.clone(),
                    author: c.author.clone(),
                    committed_at: c.committed_at.to_string(),
                    paths: c.paths.clone(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DriftCommit, Report, Summary};
    use crate::mode::Mode;
    use jiff::Timestamp;

    fn commit(sha: &str, author: &str, second: i64) -> DriftCommit {
        DriftCommit {
            sha: sha.to_string(),
            author: author.to_string(),
            committed_at: Timestamp::from_second(second).expect("in range"),
            paths: vec!["src/main.rs".to_string()],
        }
    }

    fn summary(commits: Vec<DriftCommit>) -> Summary {
        Summary {
            from: "a".into(),
            to: "b".into(),
            mode: Mode::Log,
            driftignore_present: true,
            commits_scanned: 3,
            merge_commits_skipped: 1,
            diverged: false,
            commits,
            unattributed_paths: Vec::new(),
        }
    }

    fn now() -> Timestamp {
        Timestamp::from_second(1_000).expect("in range")
    }

    #[test]
    fn no_commits_means_no_drift() {
        let report = Report::new(summary(vec![]), now());
        assert!(!report.drift_detected);
        assert!(report.oldest_drift.is_none());
        assert!(report.newest_drift.is_none());
        assert!(report.drift_paths.is_empty());
    }

    #[test]
    fn bounds_use_the_oldest_and_newest_commit() {
        let report = Report::new(
            summary(vec![
                commit("old", "Ann <ann@example.com>", 100),
                commit("new", "Bob <bob@example.com>", 900),
            ]),
            now(),
        );
        assert!(report.drift_detected);
        assert_eq!(report.drift_commits, vec!["new", "old"]);
        assert_eq!(report.oldest_drift.expect("set").age_seconds, 900);
        assert_eq!(report.newest_drift.expect("set").age_seconds, 100);
    }

    #[test]
    fn authors_and_paths_are_distinct_and_sorted() {
        let mut commits = vec![
            commit("one", "Bob <bob@example.com>", 100),
            commit("two", "Ann <ann@example.com>", 200),
            commit("three", "Bob <bob@example.com>", 300),
        ];
        commits[1].paths = vec!["Cargo.toml".to_string()];

        let report = Report::new(summary(commits), now());
        assert_eq!(
            report.drift_authors,
            vec!["Ann <ann@example.com>", "Bob <bob@example.com>"]
        );
        assert_eq!(report.drift_paths, vec!["Cargo.toml", "src/main.rs"]);
    }

    #[test]
    fn an_unattributed_path_is_still_drift() {
        let mut summary = summary(vec![]);
        summary.mode = Mode::Tree;
        summary.diverged = true;
        summary.unattributed_paths = vec!["deploy/prod.yaml".to_string()];

        let report = Report::new(summary, now());
        assert!(report.drift_detected);
        assert_eq!(report.drift_commit_count, 0);
        assert_eq!(report.drift_paths, vec!["deploy/prod.yaml"]);
        assert!(report.oldest_drift.is_none());
    }
}

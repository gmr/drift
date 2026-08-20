//! End to end tests over fixture repositories.
//!
//! The fixtures are built with the `git` CLI on purpose: the binary under test must
//! agree with git about what a commit range contains, so git writes the history.

use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

/// Fixed clock so commit ids and reported ages stay stable between runs.
const EPOCH: i64 = 1_700_000_000;

struct Fixture {
    dir: TempDir,
    commits: usize,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let fixture = Self { dir, commits: 0 };
        fixture.git(&["init", "--initial-branch=main"]);
        fixture.git(&["config", "user.name", "Ann Author"]);
        fixture.git(&["config", "user.email", "ann@example.com"]);
        fixture
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(self.path())
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("utf-8 output")
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.path().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, contents).expect("write file");
    }

    /// Commit everything in the worktree at a timestamp derived from the commit count.
    fn commit(&mut self, message: &str) -> String {
        self.commits += 1;
        let stamp = format!("{} +0000", EPOCH + self.commits as i64 * 60);
        self.git(&["add", "-A"]);
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(self.path())
            .env("GIT_AUTHOR_DATE", &stamp)
            .env("GIT_COMMITTER_DATE", &stamp)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        self.head()
    }

    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }

    /// Run the binary over `from..to` in the default mode and parse its JSON report.
    fn report(&self, from: &str, to: &str) -> Value {
        self.report_with(&[], from, to)
    }

    /// Run the binary with extra flags and parse its JSON report.
    fn report_with(&self, flags: &[&str], from: &str, to: &str) -> Value {
        let output = Command::cargo_bin("drift")
            .expect("binary built")
            .args(["-C", self.path().to_str().expect("utf-8 path")])
            .args(flags)
            .args([from, to])
            .output()
            .expect("drift runs");
        assert!(
            output.status.success(),
            "drift failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("json report")
    }
}

/// A repository whose `.driftignore` covers `docs/` and `CHANGELOG.md`.
fn baseline() -> (Fixture, String) {
    let mut fixture = Fixture::new();
    fixture.write(
        ".driftignore",
        "# expected to change\ndocs/\nCHANGELOG.md\n",
    );
    fixture.write("src/main.rs", "fn main() {}\n");
    let base = fixture.commit("Add the baseline");
    (fixture, base)
}

#[test]
fn ignored_paths_are_not_drift() {
    let (mut fixture, base) = baseline();
    fixture.write("docs/guide.md", "a guide\n");
    fixture.write("CHANGELOG.md", "released\n");
    let head = fixture.commit("Update the docs");

    let report = fixture.report(&base, &head);
    assert_eq!(report["drift_detected"], false);
    assert_eq!(report["commits_scanned"], 1);
    assert_eq!(report["driftignore_present"], true);
    assert!(report["oldest_drift"].is_null());
}

#[test]
fn an_unignored_path_is_drift() {
    let (mut fixture, base) = baseline();
    fixture.write("docs/guide.md", "a guide\n");
    fixture.commit("Update the docs");
    fixture.write("src/lib.rs", "pub fn drift() {}\n");
    let head = fixture.commit("Add a library");

    let report = fixture.report(&base, &head);
    assert_eq!(report["drift_detected"], true);
    assert_eq!(report["commits_scanned"], 2);
    assert_eq!(report["drift_commit_count"], 1);
    assert_eq!(report["drift_commits"][0], head);
    assert_eq!(report["drift_authors"][0], "Ann Author <ann@example.com>");
    assert_eq!(report["details"][0]["paths"][0], "src/lib.rs");
}

#[test]
fn tags_resolve_as_refs() {
    let (mut fixture, _base) = baseline();
    fixture.git(&["tag", "v1"]);
    fixture.write("src/lib.rs", "pub fn drift() {}\n");
    let head = fixture.commit("Add a library");

    let report = fixture.report("v1", "HEAD");
    assert_eq!(report["drift_detected"], true);
    assert_eq!(report["to"], head);
}

#[test]
fn ages_run_from_oldest_to_newest() {
    let (mut fixture, base) = baseline();
    fixture.write("src/one.rs", "// one\n");
    let first = fixture.commit("Add one");
    fixture.write("src/two.rs", "// two\n");
    let second = fixture.commit("Add two");

    let report = fixture.report(&base, &second);
    assert_eq!(report["oldest_drift"]["sha"], first);
    assert_eq!(report["newest_drift"]["sha"], second);
    let oldest = report["oldest_drift"]["age_seconds"].as_i64().expect("age");
    let newest = report["newest_drift"]["age_seconds"].as_i64().expect("age");
    assert_eq!(oldest - newest, 60);
}

#[test]
fn a_rename_reports_both_paths() {
    let (mut fixture, base) = baseline();
    fixture.git(&["mv", "src/main.rs", "src/entry.rs"]);
    let head = fixture.commit("Rename the entry point");

    let report = fixture.report(&base, &head);
    assert_eq!(report["drift_detected"], true);
    assert_eq!(report["details"][0]["paths"][0], "src/entry.rs");
    assert_eq!(report["details"][0]["paths"][1], "src/main.rs");
}

#[test]
fn a_rename_into_an_ignored_path_still_drifts_at_the_source() {
    let (mut fixture, base) = baseline();
    fixture.write("docs/.keep", "");
    fixture.commit("Add the docs directory");
    fixture.git(&["mv", "src/main.rs", "docs/main.rs"]);
    let head = fixture.commit("Move the entry point into docs");

    let report = fixture.report(&base, &head);
    assert_eq!(report["drift_detected"], true);
    assert_eq!(report["drift_commits"][0], head);
    assert_eq!(
        report["details"][0]["paths"],
        serde_json::json!(["src/main.rs"])
    );
}

#[test]
fn a_drift_ignore_trailer_excludes_its_commit() {
    let (mut fixture, base) = baseline();
    fixture.write("src/main.rs", "fn main() { println!(); }\n");
    fixture.commit("Reformat main\n\nDrift: ignore");
    fixture.write("src/other.rs", "// other\n");
    let head = fixture.commit("Add other file");

    let report = fixture.report(&base, &head);
    assert_eq!(report["ignored_commits_skipped"], 1);
    assert_eq!(report["commits_scanned"], 1);
    assert_eq!(report["drift_commits"], serde_json::json!([head]));
}

#[test]
fn a_drift_ignore_trailer_matches_regardless_of_case() {
    let (mut fixture, base) = baseline();
    fixture.write("src/main.rs", "fn main() { println!(); }\n");
    fixture.commit("Reformat main\n\ndrift: IGNORE");
    let head = fixture.head();

    let report = fixture.report(&base, &head);
    assert_eq!(report["ignored_commits_skipped"], 1);
    assert_eq!(report["commits_scanned"], 0);
}

#[test]
fn a_drift_ignore_commit_that_writes_its_own_content_leaves_an_unattributed_path() {
    let (mut fixture, base) = baseline();
    fixture.write("src/main.rs", "fn main() { println!(); }\n");
    let head = fixture.commit("Reformat main\n\nDrift: ignore");

    let report = fixture.report_with(&["--tree"], &base, &head);
    assert_eq!(report["ignored_commits_skipped"], 1);
    assert_eq!(report["drift_commit_count"], 0);
    assert_eq!(
        report["unattributed_paths"],
        serde_json::json!(["src/main.rs"])
    );
}

#[test]
fn merges_are_skipped_but_their_commits_are_scanned() {
    let (mut fixture, base) = baseline();
    fixture.git(&["checkout", "-b", "side"]);
    fixture.write("src/side.rs", "// side\n");
    let side = fixture.commit("Add the side file");
    fixture.git(&["checkout", "main"]);
    fixture.write("docs/main.md", "main docs\n");
    fixture.commit("Add main docs");
    fixture.git(&["merge", "--no-ff", "-m", "Merge side", "side"]);
    let head = fixture.head();

    let report = fixture.report(&base, &head);
    assert_eq!(report["merge_commits_skipped"], 1);
    assert_eq!(report["commits_scanned"], 2);
    assert_eq!(report["drift_commit_count"], 1);
    assert_eq!(report["drift_commits"][0], side);
}

#[test]
fn a_merge_that_writes_its_own_content_leaves_an_unattributed_path() {
    let (mut fixture, base) = baseline();
    fixture.git(&["checkout", "-b", "side"]);
    fixture.write("src/side.rs", "// side\n");
    let side = fixture.commit("Add the side file");
    fixture.git(&["checkout", "main"]);
    fixture.write("src/main.rs", "fn main() { drift() }\n");
    let touched = fixture.commit("Touch main");

    // An evil merge: content that is in neither parent, written into the merge commit
    // the way a conflict resolution is.
    fixture.git(&["merge", "--no-ff", "--no-commit", "side"]);
    fixture.write("src/resolved.rs", "// written by the merge itself\n");
    let head = fixture.commit("Merge side and resolve");

    // base is an ancestor of head, so nothing diverged, yet the merge's own file has no
    // commit to carry it because merges are never classified.
    let report = fixture.report_with(&["--tree"], &base, &head);
    assert_eq!(report["diverged"], false);
    assert_eq!(report["merge_commits_skipped"], 1);
    assert_eq!(
        report["unattributed_paths"],
        serde_json::json!(["src/resolved.rs"])
    );
    // Both non-merge commits still report their own paths, newest first.
    assert_eq!(report["drift_commits"], serde_json::json!([touched, side]));
}

#[test]
fn a_missing_driftignore_makes_every_change_drift() {
    let mut fixture = Fixture::new();
    fixture.write("README.md", "hello\n");
    let base = fixture.commit("Add a readme");
    fixture.write("docs/guide.md", "a guide\n");
    let head = fixture.commit("Add a guide");

    let report = fixture.report(&base, &head);
    assert_eq!(report["driftignore_present"], false);
    assert_eq!(report["drift_detected"], true);
}

#[test]
fn the_driftignore_of_the_newer_ref_wins() {
    let (mut fixture, base) = baseline();
    fixture.write("src/lib.rs", "pub fn drift() {}\n");
    fixture.commit("Add a library");
    fixture.write(".driftignore", "docs/\nCHANGELOG.md\nsrc/lib.rs\n");
    let head = fixture.commit("Ignore the library");

    // The last commit changes .driftignore itself, which nothing ignores.
    let report = fixture.report(&base, &head);
    assert_eq!(report["drift_commit_count"], 1);
    assert_eq!(report["drift_commits"][0], head);
    assert_eq!(
        report["details"][0]["paths"],
        serde_json::json!([".driftignore"])
    );
}

#[test]
fn an_empty_range_reports_no_drift() {
    let (fixture, base) = baseline();
    let report = fixture.report(&base, &base);
    assert_eq!(report["commits_scanned"], 0);
    assert_eq!(report["drift_detected"], false);
}

#[test]
fn fail_on_drift_sets_the_exit_code() {
    let (mut fixture, base) = baseline();
    fixture.write("src/lib.rs", "pub fn drift() {}\n");
    let head = fixture.commit("Add a library");

    let output = Command::cargo_bin("drift")
        .expect("binary built")
        .args([
            "-C",
            fixture.path().to_str().expect("utf-8 path"),
            "--fail-on-drift",
            &base,
            &head,
        ])
        .output()
        .expect("drift runs");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn an_unknown_ref_fails_with_a_message() {
    let (fixture, _base) = baseline();
    let output = Command::cargo_bin("drift")
        .expect("binary built")
        .args([
            "-C",
            fixture.path().to_str().expect("utf-8 path"),
            "nope",
            "HEAD",
        ])
        .output()
        .expect("drift runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot resolve `nope`"), "{stderr}");
}

#[test]
fn log_mode_counts_a_change_that_was_reverted() {
    let (mut fixture, base) = baseline();
    fixture.write("src/main.rs", "fn main() { todo!() }\n");
    let broke = fixture.commit("Break the entry point");
    fixture.write("src/main.rs", "fn main() {}\n");
    let fixed = fixture.commit("Put it back");

    let report = fixture.report(&base, &fixed);
    assert_eq!(report["mode"], "log");
    assert_eq!(report["drift_commit_count"], 2);
    assert_eq!(report["drift_commits"], serde_json::json!([fixed, broke]));
}

#[test]
fn tree_mode_drops_a_change_that_was_reverted() {
    let (mut fixture, base) = baseline();
    fixture.write("src/main.rs", "fn main() { todo!() }\n");
    fixture.commit("Break the entry point");
    fixture.write("src/main.rs", "fn main() {}\n");
    let head = fixture.commit("Put it back");

    let report = fixture.report_with(&["--tree"], &base, &head);
    assert_eq!(report["mode"], "tree");
    assert_eq!(report["drift_detected"], false);
    assert_eq!(report["commits_scanned"], 2);
    assert_eq!(report["drift_commit_count"], 0);
    assert!(report["drift_paths"].as_array().expect("array").is_empty());
}

#[test]
fn tree_mode_still_dates_a_path_that_really_differs() {
    let (mut fixture, base) = baseline();
    fixture.write("src/one.rs", "// one\n");
    let first = fixture.commit("Add one");
    fixture.write("src/two.rs", "// two\n");
    fixture.write("src/two.rs", "// two\n");
    let second = fixture.commit("Add two");

    let report = fixture.report_with(&["--tree"], &base, &second);
    assert_eq!(report["drift_detected"], true);
    assert_eq!(report["oldest_drift"]["sha"], first);
    assert_eq!(report["newest_drift"]["sha"], second);
    assert_eq!(
        report["drift_paths"],
        serde_json::json!(["src/one.rs", "src/two.rs"])
    );
    assert!(
        report["unattributed_paths"]
            .as_array()
            .expect("array")
            .is_empty()
    );
}

#[test]
fn the_log_flag_is_the_default_mode() {
    let (mut fixture, base) = baseline();
    fixture.write("src/lib.rs", "pub fn drift() {}\n");
    let head = fixture.commit("Add a library");

    let mut explicit = fixture.report_with(&["--log"], &base, &head);
    let mut default = fixture.report(&base, &head);

    // Each run takes its own clock reading, so an age can differ by a second between
    // them. Everything the mode decides is what has to match.
    for report in [&mut explicit, &mut default] {
        for bound in ["oldest_drift", "newest_drift"] {
            report[bound]["age_seconds"] = Value::Null;
        }
    }

    assert_eq!(explicit, default);
    assert_eq!(explicit["mode"], "log");
}

#[test]
fn the_two_modes_conflict() {
    let (fixture, base) = baseline();
    let output = Command::cargo_bin("drift")
        .expect("binary built")
        .args([
            "-C",
            fixture.path().to_str().expect("utf-8 path"),
            "--log",
            "--tree",
            &base,
            "HEAD",
        ])
        .output()
        .expect("drift runs");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot be used with"), "{stderr}");
}

#[test]
fn diverged_refs_are_flagged() {
    let (mut fixture, base) = baseline();
    fixture.git(&["checkout", "-b", "staging"]);
    fixture.write("hotfix.txt", "urgent\n");
    fixture.commit("Hotfix straight onto staging");
    fixture.git(&["checkout", "main"]);
    fixture.write("src/lib.rs", "pub fn drift() {}\n");
    let head = fixture.commit("Add a library");

    // main is not a descendant of staging: each side moved after they last agreed.
    let report = fixture.report_with(&["--tree"], "staging", &head);
    assert_eq!(report["diverged"], true);
    assert_eq!(
        report["from"],
        fixture.git(&["rev-parse", "staging"]).trim()
    );

    // The hotfix only staging has still differs between the trees, and no commit in
    // staging..main explains it.
    assert_eq!(
        report["unattributed_paths"],
        serde_json::json!(["hotfix.txt"])
    );
    assert_eq!(report["drift_commits"][0], head);
    assert_eq!(report["drift_detected"], true);

    // A clean promotion of the same range is not diverged.
    let clean = fixture.report_with(&["--tree"], &base, &head);
    assert_eq!(clean["diverged"], false);
    assert!(
        clean["unattributed_paths"]
            .as_array()
            .expect("array")
            .is_empty()
    );
}

#[test]
fn an_unattributed_path_alone_still_reports_drift() {
    let (mut fixture, _base) = baseline();
    fixture.git(&["checkout", "-b", "staging"]);
    fixture.write("hotfix.txt", "urgent\n");
    fixture.commit("Hotfix straight onto staging");
    fixture.git(&["checkout", "main"]);

    // main has nothing staging lacks, yet the trees differ.
    let report = fixture.report_with(&["--tree"], "staging", "main");
    assert_eq!(report["drift_detected"], true);
    assert_eq!(report["commits_scanned"], 0);
    assert_eq!(report["drift_commit_count"], 0);
    assert_eq!(
        report["unattributed_paths"],
        serde_json::json!(["hotfix.txt"])
    );
    assert!(report["oldest_drift"].is_null());
}

use std::collections::BTreeSet;
use std::path::Path;

use gix::object::tree::diff::ChangeDetached;
use jiff::Timestamp;

use crate::driftignore::DriftIgnore;
use crate::error::{Error, Result, git};
use crate::mode::Mode;
use crate::report::{DriftCommit, Report, Summary};

/// Open the repository containing `path`, walking up to find the git directory.
pub fn open(path: &Path) -> Result<gix::Repository> {
    gix::discover(path).map_err(|source| Error::OpenRepository {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

/// Find the meaningful changes between two refs.
///
/// `mode` chooses what counts as a drifting path: every path the range touched
/// ([`Mode::Log`]) or only the paths whose content differs between the two refs
/// ([`Mode::Tree`]). Either way the paths `.driftignore` covers are dropped, and the
/// commits behind the surviving paths are reported with their authors and ages.
///
/// `now` is the reference point for those ages.
///
/// `.driftignore` comes from the tree of `to`, so the same pair of refs always
/// produces the same answer regardless of the worktree state.
pub fn analyze(
    repo: &gix::Repository,
    from: &str,
    to: &str,
    mode: Mode,
    now: Timestamp,
) -> Result<Report> {
    let from_commit = resolve(repo, from)?;
    let to_commit = resolve(repo, to)?;
    let driftignore = DriftIgnore::from_commit(&to_commit)?;

    // Rewrite tracking stays off so a rename is always reported as its two literal
    // paths, and paths are tracked because they are the entire point of the walk.
    let mut options = gix::diff::Options::default();
    options.track_path().track_rewrites(None);

    // The refs have diverged when neither is an ancestor of the other, which happens
    // whenever an environment branch took a change of its own. In tree mode that is
    // what leaves paths no commit in the range explains.
    let base = repo
        .merge_base(from_commit.id, to_commit.id)
        .map_err(git)?
        .detach();
    let diverged = base != from_commit.id;

    // In tree mode, the paths that still differ are the only ones worth attributing.
    let net_paths = match mode {
        Mode::Log => None,
        Mode::Tree => {
            let from_tree = from_commit.tree().map_err(git)?;
            let to_tree = to_commit.tree().map_err(git)?;
            let mut paths = BTreeSet::new();
            for path in diff_paths(repo, Some(&from_tree), &to_tree, options)? {
                if !driftignore.is_ignored(&path) {
                    paths.insert(path);
                }
            }
            Some(paths)
        }
    };

    let walk = repo
        .rev_walk([to_commit.id])
        .with_hidden([from_commit.id])
        .all()
        .map_err(git)?;

    let mut scanned = 0usize;
    let mut merges = 0usize;
    let mut commits = Vec::new();
    let mut attributed = BTreeSet::new();

    for info in walk {
        let info = info.map_err(git)?;
        if info.parent_ids().count() > 1 {
            // A merge introduces no change of its own; its side is already covered by
            // the commits the walk reaches through it.
            merges += 1;
            continue;
        }
        scanned += 1;

        let commit = info.object().map_err(git)?;
        let changed = changed_paths(repo, &commit, options)?;
        let drifting: Vec<String> = changed
            .into_iter()
            .filter(|path| match &net_paths {
                // Tree mode ignores a path the two refs agree on, however often the
                // range touched it, and `.driftignore` was already applied to the set.
                Some(net) => net.contains(path),
                None => !driftignore.is_ignored(path),
            })
            .collect();
        if drifting.is_empty() {
            continue;
        }
        attributed.extend(drifting.iter().cloned());

        let author = commit.author().map_err(git)?;
        let time = commit.time().map_err(git)?;
        commits.push(DriftCommit {
            sha: commit.id().to_hex().to_string(),
            author: format!("{} <{}>", author.name, author.email),
            committed_at: Timestamp::from_second(time.seconds)
                .map_err(|source| Error::Git(Box::new(source) as crate::error::BoxedError))?,
            paths: drifting,
        });
    }

    // A drifting path with no commit behind it comes from the `from` side, so nothing
    // in `from..to` can date it.
    let unattributed_paths = net_paths
        .map(|net| net.difference(&attributed).cloned().collect())
        .unwrap_or_default();

    Ok(Report::new(
        Summary {
            from: from_commit.id().to_hex().to_string(),
            to: to_commit.id().to_hex().to_string(),
            mode,
            driftignore_present: driftignore.is_present(),
            commits_scanned: scanned,
            merge_commits_skipped: merges,
            diverged,
            commits,
            unattributed_paths,
        },
        now,
    ))
}

/// Resolve any committish, a sha, tag, branch, or `HEAD~2`, to one commit.
fn resolve<'repo>(repo: &'repo gix::Repository, rev: &str) -> Result<gix::Commit<'repo>> {
    repo.rev_parse_single(rev)
        .map_err(|source| Error::ResolveRev {
            rev: rev.to_string(),
            source: Box::new(source),
        })?
        .object()
        .map_err(git)?
        .peel_to_commit()
        .map_err(|source| Error::ResolveRev {
            rev: rev.to_string(),
            source: Box::new(source),
        })
}

/// The file paths a commit changed relative to its first parent.
///
/// A root commit is diffed against the empty tree, so every file it adds counts.
fn changed_paths(
    repo: &gix::Repository,
    commit: &gix::Commit<'_>,
    options: gix::diff::Options,
) -> Result<Vec<String>> {
    let new_tree = commit.tree().map_err(git)?;
    let old_tree = match commit.parent_ids().next() {
        Some(id) => Some(
            id.object()
                .map_err(git)?
                .peel_to_commit()
                .map_err(git)?
                .tree()
                .map_err(git)?,
        ),
        None => None,
    };
    diff_paths(repo, old_tree.as_ref(), &new_tree, options)
}

/// The file paths that differ between two trees.
///
/// Directory entries are dropped because the diff also yields every file below them.
fn diff_paths(
    repo: &gix::Repository,
    old_tree: Option<&gix::Tree<'_>>,
    new_tree: &gix::Tree<'_>,
    options: gix::diff::Options,
) -> Result<Vec<String>> {
    let changes = repo
        .diff_tree_to_tree(old_tree, Some(new_tree), options)
        .map_err(git)?;

    let mut paths = Vec::with_capacity(changes.len());
    for change in &changes {
        if change.entry_mode().is_tree() {
            continue;
        }
        paths.push(change.location().to_string());
        if let ChangeDetached::Rewrite {
            source_location, ..
        } = change
        {
            // Unreachable while rewrite tracking is off, kept so enabling it cannot
            // silently drop the source side of a rename.
            paths.push(source_location.to_string());
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

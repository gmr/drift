# drift

A CLI tool for git repositories that determines if there are changes between two
git refs that you should care about.

```sh
drift 1.1.0 main    # does main hold anything the deployed tag does not?
drift 1.1.0 1.1.1   # and what did the last release actually carry?
```

`.driftignore` lists the paths whose changes do not matter. Everything else does.
`drift` compares two refs, drops the noise, and reports what is left: the changes
worth acting on, who made them, and how long they have been waiting.

The output is JSON, on stdout, one object per run. It is meant to be piped into
`jq`, a CI step, or an alert.

## Why

In managing deployments across environments, it can be useful to know if there
are meaningful changes in the codebase that have not made it to the production.
Determining what constitutes drift can be challenging, for example if you have
a documentation-only change in the repo, do you really care if the application
has been deployed based on that commit?

`drift` is a tool to detect if the things you care about in a repo have changed.
For example you may care if `uv.lock` changes but not `pyproject.toml`, as an
updated lockfile can indicate a change in dependencies that should be deployed,
such as security fixes.

`drift` makes divergence something you can identify. List the paths you do not
care about in `.driftignore`, and every commit that touched anything else gets
reported with its sha, its author, and how old it is.

`git diff --stat` answers "what is different". `drift` answers "is any of it worth
a deploy, and how long has it been waiting".

## Installation

### Homebrew (macOS / Linux)

```bash
brew install gmr/utils/drift
```

> [!NOTE]
> Homebrew 6.0 added [tap trust](https://docs.brew.sh/Tap-Trust), and some
> versions fail to install third-party taps inside the build sandbox (the
> error mentions `build.rb ... exited with 1`). If you hit this, trust the
> formula first:
>
> ```bash
> brew trust --formula gmr/utils/drift
> ```
>
> or, as a temporary workaround, set `HOMEBREW_NO_REQUIRE_TAP_TRUST=1` for
> the install.

### Quick Install (Linux / macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/gmr/drift/main/install.sh | sh
```

To install to a custom directory:

```bash
INSTALL_DIR=~/.local/bin curl -fsSL https://raw.githubusercontent.com/gmr/drift/main/install.sh | sh
```

### Install a Specific Version

```bash
VERSION=v1.0.0 curl -fsSL https://raw.githubusercontent.com/gmr/drift/main/install.sh | sh
```

### From Source (via Cargo)

```bash
cargo install --git https://github.com/gmr/drift
```

### Download Binaries

Pre-built binaries for Linux and macOS (x86_64 and aarch64) are available on the
[GitHub Releases](https://github.com/gmr/drift/releases) page.

### Docker

Multi-arch container images (`linux/amd64`, `linux/arm64`) are published to the
GitHub Container Registry:

```bash
docker pull ghcr.io/gmr/drift:latest
```

Available tags:

- `latest` — most recent commit on `main`
- `main` — alias for `latest`
- `<version>`, `<major>.<minor>`, `<major>` — tagged releases (e.g. `1.1.0`, `1.1`, `1`)

The binary is installed at `/usr/local/bin/drift` and is the image's entrypoint, so
arguments go straight to `docker run`. Mount the repository and set the working
directory to it:

```bash
docker run --rm -v "$PWD:/repo:ro" -w /repo ghcr.io/gmr/drift:latest --tree 1.1.0 main
```

The mount can be read-only: `drift` never writes to the repository. It does need
real history, so a shallow clone will fail to resolve the older ref.

The image runs as an unprivileged user, uid 65532, not as root. A bind-mounted
repository therefore has to be readable by that uid. A normal checkout is, but if
yours is not, run as yourself instead:

```bash
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/repo:ro" -w /repo \
  ghcr.io/gmr/drift:latest --tree 1.1.0 main
```

#### Copy the binary into your own image

The binary is statically linked against musl and depends on nothing at runtime, not
even `git`, so it can be copied into any base image with a `COPY --from=` directive.
This is the recommended pattern: your image keeps its own base, and `drift` rides
along.

```dockerfile
FROM python:3.13-slim

COPY --from=ghcr.io/gmr/drift:latest /usr/local/bin/drift /usr/local/bin/drift

COPY my-app /usr/local/bin/my-app
```

That works in glibc images as well as musl ones, because nothing is dynamically
linked.

#### Use it as a base image

If you have no base image preference, build `FROM` it directly. It is Alpine plus
the binary, around 14 MB.

```dockerfile
FROM ghcr.io/gmr/drift:latest
COPY my-script.sh /usr/local/bin/my-script.sh
ENTRYPOINT ["my-script.sh"]
```

Everything runs in-process through
[gix](https://github.com/GitoxideLabs/gitoxide), so no installation of `drift`
needs `git` on `PATH`.

## Usage

```text
drift [OPTIONS] <FROM> <TO>

Arguments:
  <FROM>  The older ref, excluded from the range
  <TO>    The newer ref, included in the range, and the source of .driftignore

Options:
  -C, --repo <PATH>    Repository to inspect [default: .]
      --log            Count every path the range touched, even if it was
                       changed back [default]
      --tree           Count only the paths whose content differs between the
                       two refs
      --pretty         Indent the JSON output
      --fail-on-drift  Exit 1 when drift is detected
  -h, --help           Print help
  -V, --version        Print version
```

Both refs are anything git accepts as a committish: a full or short sha, a tag, a
branch, `origin/main`, `HEAD~3`.

```sh
# Is what is deployed behind what is on main?
drift 1.1.0 main

# What did one release carry over the one before it?
drift 1.1.0 1.1.1

# Since the last release, whatever it was
drift "$(git describe --tags --abbrev=0)" HEAD

# In another checkout
drift -C ~/src/infra 2.3.0 2.4.0
```

### The two modes

Both modes filter through `.driftignore` and both report commits, authors, and
ages. They differ in which paths they consider.

`--log`, the default, counts **every path the range touched**. If a file was
changed and changed back, both commits are still reported: the work happened, and
somebody may need to know it happened.

`--tree` counts **only the paths whose content actually differs** between the two
refs, then attributes each of those paths to the commits that touched it. Churn
that cancels out drops away.

For "is a release worth cutting", `--tree` is the honest answer, because it
describes what the new tag would actually carry over the old one. For "what has
happened since the last release", `--log` is, because it does not hide work behind
a revert.

```console
$ drift --log 1.1.0 main   | jq '{drift_commit_count, drift_paths}'
{ "drift_commit_count": 4, "drift_paths": ["src/app.py", "src/util.py"] }

$ drift --tree 1.1.0 main  | jq '{drift_commit_count, drift_paths}'
{ "drift_commit_count": 1, "drift_paths": ["src/app.py"] }
```

Here three of the four commits churned `src/util.py` back to where it started, so
a tag cut from `main` would carry exactly one meaningful file.

### Diverged refs

Not every tag sits on the line of the branch you are comparing it to. A `1.1.1`
cut from a release branch, with a hotfix that was never merged back, is not an
ancestor of `main`. Whenever the older ref is not an ancestor of the newer one,
the report sets `diverged` to `true`.

In `--tree` mode a divergence can leave paths that differ but that **no commit in
the range explains**, because the difference comes from the older ref's side.
Those land in `unattributed_paths`. They still count as drift, since the two refs
genuinely differ, but nothing in `from..to` can date them, so they carry no age.
Seeing them is the signal that the tag holds work the branch never received.

`--log` mode only ever looks at commits reachable from `to`, so it never produces
unattributed paths, and it never sees a hotfix that lives only on the older ref.

### Exit codes

| Code | Meaning                                      |
| ---- | -------------------------------------------- |
| 0    | The analysis ran                             |
| 1    | Drift was detected, with `--fail-on-drift`   |
| 2    | The analysis failed; the reason is on stderr |

Without `--fail-on-drift`, detecting drift is not an error. Read
`drift_detected` from the JSON instead.

## The report

```console
$ drift 1.1.0 main --pretty
{
  "drift_detected": true,
  "mode": "log",
  "from": "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
  "to": "0f1e2d3c4b5a69788796a5b4c3d2e1f098765432",
  "driftignore_present": true,
  "diverged": false,
  "commits_scanned": 14,
  "merge_commits_skipped": 2,
  "ignored_commits_skipped": 1,
  "drift_commit_count": 2,
  "drift_commits": [
    "0f1e2d3c4b5a69788796a5b4c3d2e1f098765432",
    "9a8b7c6d5e4f30211203f4e5d6c7b8a998877665"
  ],
  "drift_authors": [
    "Ann Author <ann@example.com>",
    "Bob Builder <bob@example.com>"
  ],
  "drift_paths": [
    "src/app.py",
    "src/handlers.py",
    "uv.lock"
  ],
  "unattributed_paths": [],
  "oldest_drift": {
    "sha": "9a8b7c6d5e4f30211203f4e5d6c7b8a998877665",
    "committed_at": "2026-07-28T10:45:40Z",
    "age_seconds": 1209600
  },
  "newest_drift": {
    "sha": "0f1e2d3c4b5a69788796a5b4c3d2e1f098765432",
    "committed_at": "2026-08-10T09:15:40Z",
    "age_seconds": 91800
  },
  "details": [
    {
      "sha": "0f1e2d3c4b5a69788796a5b4c3d2e1f098765432",
      "author": "Ann Author <ann@example.com>",
      "committed_at": "2026-08-10T09:15:40Z",
      "paths": ["src/app.py", "uv.lock"]
    },
    {
      "sha": "9a8b7c6d5e4f30211203f4e5d6c7b8a998877665",
      "author": "Bob Builder <bob@example.com>",
      "committed_at": "2026-07-28T10:45:40Z",
      "paths": ["src/handlers.py"]
    }
  ]
}
```

| Field                   | Meaning                                                          |
| ----------------------- | ---------------------------------------------------------------- |
| `drift_detected`        | Whether anything worth acting on is in the range                 |
| `mode`                  | `log` or `tree`, which set of paths was considered               |
| `from`, `to`            | The resolved shas of the two refs                                |
| `driftignore_present`   | Whether the newer ref carries a `.driftignore`                   |
| `diverged`              | Whether each ref has moved since they last agreed                |
| `commits_scanned`       | Non-merge commits classified                                     |
| `merge_commits_skipped` | Merge commits traversed but not classified                       |
| `ignored_commits_skipped` | Commits with a `Drift: ignore` trailer, traversed but not classified |
| `drift_commit_count`    | How many of the scanned commits drifted                          |
| `drift_commits`         | Their shas, newest first                                         |
| `drift_authors`         | Distinct `Name <email>` of the drift commit authors, sorted      |
| `drift_paths`           | Every drifting path, sorted                                      |
| `unattributed_paths`    | Drifting paths no commit in the range explains; `tree` mode only |
| `oldest_drift`          | The earliest drift commit, with its age in seconds               |
| `newest_drift`          | The latest drift commit, with its age in seconds                 |
| `details`               | Each drift commit with the paths that caused it, newest first    |

`oldest_drift` and `newest_drift` are `null` when no commit in the range drifted.
`oldest_drift.age_seconds` is the one to alert on: it says how long the change has
been waiting, not how recently someone touched it.

Adding a field is not a breaking change, so read by name rather than by shape.

### Reading it with jq

```sh
# One line per drift commit
drift 1.1.0 main | jq -r '.details[] | "\(.sha[0:8]) \(.author) \(.paths | join(", "))"'

# How many days has the deployed tag been behind?
drift 1.1.0 main | jq '(.oldest_drift.age_seconds // 0) / 86400 | floor'

# Who to ask about it
drift 1.1.0 main | jq -r '.drift_authors[]'

# Is a release worth cutting at all?
drift --tree 1.1.0 main | jq -e '.drift_detected' >/dev/null && echo "cut it"
```

## .driftignore

`.driftignore` is the noise floor: the paths whose changes you do not want to hear
about. It uses gitignore pattern syntax.

```gitignore
# Documentation only, never worth a deploy
docs/
*.md

# Release automation owns these
CHANGELOG.md
/version.txt

# Generated
**/*.pb.go

# A dependency change is worth knowing about, the manifest churn is not
pyproject.toml
```

Note which side of the line `uv.lock` is on: it is *not* listed, because an updated
lock file can mean a security fix that should ship, while the `pyproject.toml` edit
that produced it says nothing on its own.

Patterns use gitignore syntax and are evaluated against paths relative to the
repository root. A pattern containing a slash is anchored; one without a slash
matches that name at any depth:

| Pattern          | Matches                                       |
| ---------------- | --------------------------------------------- |
| `CHANGELOG.md`   | That name at any depth                        |
| `/version.txt`   | Only at the repository root                   |
| `docs/`          | That directory and everything under it        |
| `**/*.pb.go`     | That suffix at any depth                      |
| `!docs/api/x.md` | Re-includes a path an earlier pattern ignored |

One deliberate difference from git: a negated pattern re-includes a path even when
an earlier pattern excluded its parent directory, so `docs/api/` followed by
`!docs/api/overview.md` reports drift on that one file. Git ignores the negation
in that position and never looks inside an excluded directory. Every other
pattern behaves as it does in `.gitignore`.

The file is read **from the tree of the newer ref**, never from the worktree or
from disk. Two runs over the same range therefore always agree, even from a dirty
checkout, a bare clone, or a different machine.

A repository with no `.driftignore` at the newer ref reports every changed path
as drift, and sets `driftignore_present` to `false` so you can tell that case
apart from a repository that genuinely drifted everywhere.

Note that `.driftignore` does not ignore itself. Editing it is a change to the
repository like any other, and shows up as drift unless you list it.

## Defined behavior

The rules below are the contract. They are covered by tests in `tests/cli.rs`.

**Range.** `from..to`: `from` is excluded, `to` is included, along with
everything reachable from `to` but not from `from`. This is the set
`git log from..to` lists.

**Modes.** `--log` considers every path the range touched. `--tree` considers only
the paths whose content differs between the two trees, and attributes each to the
commits in the range that touched it. Both then drop the paths `.driftignore`
covers, and both report the same fields. The two flags conflict; `--log` is the
default.

**Divergence.** `diverged` is true when the merge base of the two refs is not
`from`, meaning each ref has moved since they last agreed. In `--tree` mode the
paths that differ because of the `from` side appear in `unattributed_paths`, with
no commit and no age, because no commit in `from..to` explains them. Swapping the
two refs is the usual cause; the report saying `diverged` with everything
unattributed is what that mistake looks like.

**Unattributed paths.** `unattributed_paths` holds the `--tree` mode paths that
differ between the two trees and that no classified commit in the range touched.
Divergence is one cause. A merge commit that wrote content of its own is the
other, and it needs no divergence: the merge is never classified, so its
resolution has no commit to carry it.

**Merges.** Merge commits, meaning commits with more than one parent, are
traversed but never classified. A merge carries no change of its own that is not
already in a commit on one of its sides, and the walk reaches those commits
through it. They are counted under `merge_commits_skipped`. A conflict resolution
written into the merge commit itself is therefore never attributed to a commit:
`--log` mode does not report it at all, and `--tree` mode reports the path under
`unattributed_paths`. Make it a separate commit if it needs an author and an age.

**Trailers.** A non-merge commit whose message carries a `Drift: ignore` trailer is
traversed but never classified, the same as a merge: it is counted under
`ignored_commits_skipped` instead of `commits_scanned`, and `--log` mode does not
report it. In `--tree` mode a path it alone changed has no commit to carry it and
reports under `unattributed_paths`.

**Root commits.** A commit with no parent is diffed against the empty tree, so
every file it adds counts.

**Renames.** Rename detection is off. A rename is two paths, a deletion of the
old and an addition of the new, and both are matched against `.driftignore`.
Moving a file into an ignored directory still reports drift at the source path.
This keeps the result independent of similarity thresholds and of local diff
configuration.

**Paths.** Only file paths are examined. Directory entries in the diff are
dropped, because the diff also yields every file below them. A path that is not
valid UTF-8 is an error rather than a lossy conversion, so two different paths can
never collapse into one report entry.

**Ages.** Both ages are the seconds between the commit's _committer_ timestamp
and the moment the run started. A single clock reading is used for both, so
`oldest_drift` and `newest_drift` are always consistent with each other. Author
dates are ignored, because git orders history by committer date.

**Authors.** `drift_authors` holds the distinct `Name <email>` of each commit's
_author_, sorted. No mailmap is applied.

**Ordering.** `drift_commits` and `details` run newest first, ties broken by sha,
and every path list is sorted, so repeated runs produce byte for byte identical
output.

## In CI

Skip cutting a release when the deployed tag is not missing anything:

```yaml
- uses: actions/checkout@v5
  with:
    fetch-depth: 0 # drift needs the history, not just the tip
- id: drift
  env:
    DEPLOYED_TAG: ${{ vars.DEPLOYED_TAG }}
  run: |
    {
      echo "report<<EOF"
      drift --tree "$DEPLOYED_TAG" HEAD
      echo "EOF"
    } >> "$GITHUB_OUTPUT"
- if: fromJSON(steps.drift.outputs.report).drift_detected
  run: ./release.sh
```

Or fail a pull request that adds drift on top of the last release:

```yaml
- run: drift --fail-on-drift "$(git describe --tags --abbrev=0)" HEAD
```

`drift` never reads a tag's own name for meaning, so a `v` prefix or its absence
does not matter; both refs are resolved the way git resolves them.

A shallow clone is the usual cause of a surprising result: if `from` is not in
the fetched history, the ref cannot be resolved and `drift` exits 2.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The end to end tests build fixture repositories with the `git` CLI on purpose:
git, not this crate, decides what a commit range contains.

## License

BSD 3-Clause. See [LICENSE](LICENSE).

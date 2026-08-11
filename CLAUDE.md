# CLAUDE.md

`drift` reports commits between two git refs that changed paths `.driftignore`
does not cover. See [README.md](README.md) for the user facing contract, and
treat the "Defined behavior" section there as the specification: any change to
range, merge, rename, age, or ordering semantics updates that section in the
same commit.

## Layout

| Path                 | Holds                                                     |
| -------------------- | --------------------------------------------------------- |
| `src/main.rs`        | The CLI: argument parsing, JSON printing, exit codes      |
| `src/scan.rs`        | The commit walk and the per-commit path resolution         |
| `src/mode.rs`        | `--log` vs `--tree`: which paths count as drifting          |
| `src/driftignore.rs` | Reading and matching `.driftignore`                        |
| `src/report.rs`      | The report structure and its JSON shape                    |
| `src/error.rs`       | One error enum for the whole crate                         |
| `tests/cli.rs`       | End to end tests over fixture repositories                 |

## Rules

- No `git` subprocess in `src/`. Everything goes through `gix`. The tests build
  their fixtures with the `git` CLI on purpose, so that git, not this crate,
  decides what a commit range contains.
- The JSON report is the interface. Adding a field is fine; renaming or removing
  one is a breaking change.
- `.driftignore` is read from the tree of the newer ref, never from disk. A run
  must not depend on the worktree.
- Keep the output deterministic. Sort anything that reaches the report.

## Commands

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

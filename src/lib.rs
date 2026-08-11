//! Find the meaningful changes between two git refs.
//!
//! A path drifts when it changed and `.driftignore` does not cover it. The commits
//! behind the drifting paths carry the answer to "how long has this been pending".
//! Everything runs in-process through `gix`; no `git` binary is invoked.

pub mod driftignore;
pub mod error;
pub mod mode;
pub mod report;
pub mod scan;

pub use driftignore::DriftIgnore;
pub use error::{Error, Result};
pub use mode::Mode;
pub use report::{DriftBound, DriftCommit, DriftDetail, Report};
pub use scan::{analyze, open};

use serde::Serialize;

/// How a range is reduced to a set of drifting paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Every path any commit in the range touched.
    ///
    /// A path that changed and was changed back still counts, because the work
    /// still happened. This is the history of the range.
    #[default]
    Log,

    /// Only the paths whose content actually differs between the two refs.
    ///
    /// A change that was reverted inside the range drops out. This is the state of
    /// the two refs, which is what a promotion between environments carries.
    Tree,
}

impl Mode {
    /// The flag name, for messages.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Log => "log",
            Mode::Tree => "tree",
        }
    }
}

//! The startup report: for each input and output, what was considered and why each option was
//! or was not used. The app prints it, and the window shows it on hover.

/// What was done with one option the engine considered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Used,
    /// Available, but another option was better.
    NotUsed,
    /// Not usable on this system.
    Unavailable,
}

/// One option considered for an input or output, and why it was or was not used. Lets the app
/// tell the user what it picked, what else there is, and how to choose differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Considered {
    pub name: String,
    pub verdict: Verdict,
    pub why: String,
}

impl Considered {
    pub(crate) fn new(name: impl Into<String>, verdict: Verdict, why: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            verdict,
            why: why.into(),
        }
    }
}

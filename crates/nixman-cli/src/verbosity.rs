//! Verbosity level derived from -q/-v/-vv flags.

/// Verbosity level for controlling output detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub enum Verbosity {
    Quiet,
    Normal,
    Verbose,
    Trace,
}

#[allow(dead_code)]
impl Verbosity {
    pub fn from_flags(quiet: bool, verbose: u8) -> Self {
        if quiet { return Self::Quiet; }
        match verbose {
            0 => Self::Normal,
            1 => Self::Verbose,
            _ => Self::Trace,
        }
    }

    pub fn is_quiet(self) -> bool { self == Self::Quiet }
    pub fn is_verbose(self) -> bool { self >= Self::Verbose }
    pub fn is_trace(self) -> bool { self == Self::Trace }
}

//! Item identifiers and visual helpers shared by the UI and config.

/// One cell of the strip. `Disk` carries the PDH instance name (e.g. `"0 C:"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ItemId {
    Cpu,
    Ram,
    Gpu,
    Disk(String),
    ClaudeFive,
    ClaudeWeek,
    CodexFive,
    CodexWeek,
}

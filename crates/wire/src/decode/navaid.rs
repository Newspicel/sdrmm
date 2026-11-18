//! What a radio navigation aid receiver is set with, whichever aid it reads.

pub const MIN_NAVAID_REPORT_MS: u32 = 250;

pub const MAX_NAVAID_REPORT_MS: u32 = 5_000;

#[must_use]
pub const fn default_navaid_report_ms() -> u32 {
    500
}

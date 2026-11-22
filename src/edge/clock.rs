#![expect(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "this is the only module that reads the clock"
)]

use std::time::{SystemTime, UNIX_EPOCH};

use crate::pure::agent_ledger::utc_text;

// Whole seconds, as the text keeps no fraction. Before 1970 the fraction drops toward 1970, so the
// second is at most one off.
pub(crate) fn now_utc() -> String {
    let at = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => 0_i64.saturating_add_unsigned(since.as_secs()),
        Err(before) => 0_i64.saturating_sub_unsigned(before.duration().as_secs()),
    };
    utc_text(at)
}

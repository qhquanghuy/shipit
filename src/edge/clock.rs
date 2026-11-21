#![expect(
    clippy::disallowed_methods,
    reason = "this is the only module that reads the clock"
)]

pub(crate) fn now_utc() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

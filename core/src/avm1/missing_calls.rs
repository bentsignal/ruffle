//! Opt-in native diagnostics for calls AVM1 otherwise ignores silently.
//!
//! Reports only callable/API identifier names and types, with a bounded number of
//! distinct records. It does not expose argument values or executable content.

use crate::string::WStr;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

const MAX_RECORDS: usize = 256;
const MAX_NAME_UNITS: usize = 128;
static RECORDS: OnceLock<Mutex<Records>> = OnceLock::new();

fn bounded_name(name: &WStr) -> String {
    name[..name.len().min(MAX_NAME_UNITS)]
        .to_utf8_lossy()
        .into_owned()
}

pub(super) fn log_api_identifier(name: &WStr) {
    if enabled() && is_api_identifier(name) {
        log(name, "undefined");
    }
}

fn is_api_identifier(name: &WStr) -> bool {
    name.len() <= MAX_NAME_UNITS
        && (name
            .find(WStr::from_units(b"CScaleformComponent_"))
            .is_some()
            || name.ends_with(WStr::from_units(b"API")))
}

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("CSR_AVM_MISSING_CALLS").as_deref() == Ok("1"))
}

#[derive(Default)]
struct Records(HashSet<(String, &'static str)>);

impl Records {
    fn insert(&mut self, name: String, receiver_type: &'static str) -> bool {
        self.0.len() < MAX_RECORDS && self.0.insert((name, receiver_type))
    }
}

pub(super) fn log(name: &WStr, receiver_type: &'static str) {
    if !enabled() {
        return;
    }

    let name = bounded_name(name);
    let Ok(mut records) = RECORDS.get_or_init(Mutex::default).lock() else {
        return;
    };
    if records.insert(name.clone(), receiver_type) {
        tracing::warn!(name, receiver_type, "AVM1 unresolved callable");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_identifiers_match_only_the_bounded_native_patterns() {
        assert!(is_api_identifier(WStr::from_units(b"SyntheticHostAPI")));
        assert!(is_api_identifier(WStr::from_units(
            b"_global.CScaleformComponent_Test"
        )));
        assert!(!is_api_identifier(WStr::from_units(b"hitArea")));
        assert!(!is_api_identifier(WStr::from_units(b"synthetic_value")));
        let overlong = vec![b'A'; MAX_NAME_UNITS + 1];
        assert!(!is_api_identifier(WStr::from_units(overlong.as_slice())));
    }

    #[test]
    fn api_records_follow_the_process_opt_in_and_filter() {
        let matching = WStr::from_units(b"SyntheticRecordedHostAPI");
        let ignored = WStr::from_units(b"SyntheticIgnoredField");
        log_api_identifier(matching);
        log_api_identifier(ignored);
        let records = RECORDS.get().map(|records| records.lock().unwrap());
        let contains = |name| {
            records
                .as_ref()
                .is_some_and(|records| records.0.contains(&(name, "undefined")))
        };
        assert_eq!(contains("SyntheticRecordedHostAPI".into()), enabled());
        assert!(!contains("SyntheticIgnoredField".into()));
        if !enabled() {
            assert!(records.is_none());
        }
    }

    #[test]
    fn names_are_bounded_before_utf8_conversion() {
        let source = vec![0xFFFFu16; MAX_NAME_UNITS * 100];
        let name = bounded_name(WStr::from_units(source.as_slice()));
        assert_eq!(name.chars().count(), MAX_NAME_UNITS);
        assert_eq!(name.len(), MAX_NAME_UNITS * 3);
    }

    #[test]
    fn repeated_calls_do_not_consume_the_record_budget() {
        let mut records = Records::default();
        assert!(records.insert("missing".into(), "undefined"));
        assert!(!records.insert("missing".into(), "undefined"));
        assert!(records.insert("missing".into(), "null"));
        assert_eq!(records.0.len(), 2);
    }

    #[test]
    fn distinct_records_are_bounded() {
        let mut records = Records::default();
        for index in 0..MAX_RECORDS {
            assert!(records.insert(format!("missing{index}"), "undefined"));
        }
        assert!(!records.insert("beyond_limit".into(), "undefined"));
        assert_eq!(records.0.len(), MAX_RECORDS);
    }
}

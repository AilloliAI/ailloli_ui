//! Environment-variable compatibility helpers for renderer diagnostics.

use std::ffi::OsString;

/// Reads `primary`, falling back to `legacy` only when the primary is absent.
///
/// Values are kept as [`OsString`] so non-Unicode environment data remains
/// observable.
///
/// # Examples
///
/// ```
/// let value: Option<std::ffi::OsString> = std::env::var_os("AILLOLI_UI_EXAMPLE");
/// let _ = value;
/// ```
pub(crate) fn value(primary: &str, legacy: &str) -> Option<OsString> {
    std::env::var_os(primary).or_else(|| std::env::var_os(legacy))
}

/// Returns whether the selected value is `1` or ASCII-case-insensitive `true`.
///
/// Missing values and every other spelling are false.
///
/// # Examples
///
/// ```
/// assert!("TrUe".eq_ignore_ascii_case("true"));
/// assert_ne!("yes", "true");
/// ```
pub(crate) fn truthy(primary: &str, legacy: &str) -> bool {
    value(primary, legacy)
        .is_some_and(|value| value == "1" || value.to_string_lossy().eq_ignore_ascii_case("true"))
}

/// Returns whether the selected value is `0` or ASCII-case-insensitive `false`.
///
/// Missing values and every other spelling are false.
///
/// # Examples
///
/// ```
/// assert!("FALSE".eq_ignore_ascii_case("false"));
/// assert_ne!("off", "false");
/// ```
pub(crate) fn falsey(primary: &str, legacy: &str) -> bool {
    value(primary, legacy)
        .is_some_and(|value| value == "0" || value.to_string_lossy().eq_ignore_ascii_case("false"))
}

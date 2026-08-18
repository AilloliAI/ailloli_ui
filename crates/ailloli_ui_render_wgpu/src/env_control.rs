use std::ffi::OsString;

pub(crate) fn value(primary: &str, legacy: &str) -> Option<OsString> {
    std::env::var_os(primary).or_else(|| std::env::var_os(legacy))
}

pub(crate) fn truthy(primary: &str, legacy: &str) -> bool {
    value(primary, legacy)
        .is_some_and(|value| value == "1" || value.to_string_lossy().eq_ignore_ascii_case("true"))
}

pub(crate) fn falsey(primary: &str, legacy: &str) -> bool {
    value(primary, legacy)
        .is_some_and(|value| value == "0" || value.to_string_lossy().eq_ignore_ascii_case("false"))
}

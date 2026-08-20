use std::fmt;

/// Stable, provider-neutral identifier for a logical application window.
///
/// A logical window can outlive any native window or presentation surface used
/// to display it. Hosts are responsible for rejecting duplicate identifiers
/// when application windows are declared.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogicalWindowId(String);

impl LogicalWindowId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for LogicalWindowId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<String> for LogicalWindowId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for LogicalWindowId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<&String> for LogicalWindowId {
    fn from(value: &String) -> Self {
        Self::new(value.clone())
    }
}

impl AsRef<str> for LogicalWindowId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

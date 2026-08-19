use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

/// An absolute HTTP(S) URL accepted for opening outside the application.
///
/// Validation uses a standards-compliant parser, while the accepted source is
/// preserved byte-for-byte so query strings and fragments reach the system
/// opener unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExternalUrl(String);

impl ExternalUrl {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ExternalUrlError> {
        let value = value.as_ref();
        if value.is_empty() {
            return Err(ExternalUrlError::Empty);
        }
        if value.trim() != value {
            return Err(ExternalUrlError::SurroundingWhitespace);
        }

        let parsed = url::Url::parse(value).map_err(|_| ExternalUrlError::Malformed)?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(ExternalUrlError::UnsupportedScheme);
        }
        if parsed.host().is_none() {
            return Err(ExternalUrlError::MissingHost);
        }

        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ExternalUrl {
    type Error = ExternalUrlError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<String> for ExternalUrl {
    type Error = ExternalUrlError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

/// Why an external URL was rejected. Error messages intentionally omit input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalUrlError {
    Empty,
    SurroundingWhitespace,
    Malformed,
    UnsupportedScheme,
    MissingHost,
}

impl fmt::Display for ExternalUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "external URL is empty",
            Self::SurroundingWhitespace => "external URL has surrounding whitespace",
            Self::Malformed => "external URL is not a valid absolute URL",
            Self::UnsupportedScheme => "external URL scheme is not supported",
            Self::MissingHost => "external URL has no host",
        })
    }
}

impl std::error::Error for ExternalUrlError {}

/// A non-fatal failure while handing a validated URL to the host system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenUrlError {
    Unavailable,
    LaunchFailed,
}

impl fmt::Display for OpenUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("system URL opener is unavailable"),
            Self::LaunchFailed => f.write_str("system URL opener failed"),
        }
    }
}

impl std::error::Error for OpenUrlError {}

/// Provider-neutral capability used by widgets to open validated URLs.
pub trait ExternalUrlOpener {
    fn open(&self, url: &ExternalUrl) -> Result<(), OpenUrlError>;
}

/// Deterministic opener for headless runtimes and tests.
#[derive(Clone, Default)]
pub struct MemoryExternalUrlOpener {
    opened: Rc<RefCell<Vec<String>>>,
    next_error: Rc<RefCell<Option<OpenUrlError>>>,
}

impl MemoryExternalUrlOpener {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn opened_urls(&self) -> Vec<String> {
        self.opened.borrow().clone()
    }

    pub fn take_opened_urls(&self) -> Vec<String> {
        std::mem::take(&mut *self.opened.borrow_mut())
    }

    pub fn fail_next(&self, error: OpenUrlError) {
        *self.next_error.borrow_mut() = Some(error);
    }
}

impl ExternalUrlOpener for MemoryExternalUrlOpener {
    fn open(&self, url: &ExternalUrl) -> Result<(), OpenUrlError> {
        if let Some(error) = self.next_error.borrow_mut().take() {
            return Err(error);
        }
        self.opened.borrow_mut().push(url.as_str().to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_https_and_preserves_exact_source() {
        let source = "https://user:secret@example.com:8443/docs?q=token#part";
        assert_eq!(ExternalUrl::parse(source).unwrap().as_str(), source);
        assert_eq!(
            ExternalUrl::parse("http://example.com").unwrap().as_str(),
            "http://example.com"
        );
    }

    #[test]
    fn rejects_unsafe_or_non_absolute_urls() {
        for value in [
            "",
            " docs/index.html",
            "docs/index.html",
            "javascript:alert(1)",
            "data:text/plain,hello",
            "file:///tmp/secret",
            "https://",
        ] {
            assert!(ExternalUrl::parse(value).is_err(), "accepted {value:?}");
        }
    }

    #[test]
    fn validation_errors_do_not_echo_sensitive_input() {
        let secret = "secret-query-value";
        let error = ExternalUrl::parse(format!("javascript:{secret}")).unwrap_err();
        assert!(!error.to_string().contains(secret));
    }

    #[test]
    fn memory_opener_records_success_and_can_fail_once() {
        let opener = MemoryExternalUrlOpener::new();
        let url = ExternalUrl::parse("https://example.com?q=1").unwrap();
        opener.open(&url).unwrap();
        assert_eq!(opener.opened_urls(), [url.as_str()]);

        opener.fail_next(OpenUrlError::Unavailable);
        assert_eq!(opener.open(&url), Err(OpenUrlError::Unavailable));
        opener.open(&url).unwrap();
        assert_eq!(opener.opened_urls().len(), 2);
    }
}

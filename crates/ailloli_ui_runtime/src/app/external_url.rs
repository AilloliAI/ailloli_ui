//! Validated external HTTP(S) URLs and provider-neutral opening.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

/// An absolute HTTP(S) URL accepted for opening outside the application.
///
/// Validation uses a standards-compliant parser, while the accepted source is
/// preserved byte-for-byte so query strings and fragments reach the system
/// opener unchanged.
///
/// User info, ports, query strings, and fragments are allowed and may contain
/// sensitive data. Debug output includes the stored source, so callers must not
/// log values indiscriminately. Host adapters must pass the URL as one argument
/// to an opener API/process and never interpolate it into a shell command.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::ExternalUrl;
/// let source = "https://example.com:8443/docs?q=rust#api";
/// let url = ExternalUrl::parse(source)?;
/// assert_eq!(url.as_str(), source);
/// # Ok::<(), ailloli_ui_runtime::app::ExternalUrlError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExternalUrl(String);

/// Provides the operations defined for ExternalUrl.
impl ExternalUrl {
    /// Validates and preserves an absolute HTTP or HTTPS URL.
    ///
    /// Empty input and any leading/trailing Unicode whitespace are rejected
    /// before parsing. The parsed scheme must be HTTP(S) and a host must be
    /// present. The original bytes—not the parser's normalized serialization—
    /// are stored. Validation performs no DNS, network, allow-list, credential,
    /// or reachability check.
    ///
    /// # Errors
    ///
    /// Returns a specific [`ExternalUrlError`] in validation order. Errors never
    /// include the rejected source string.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{ExternalUrl, ExternalUrlError};
    /// assert!(ExternalUrl::parse("http://localhost").is_ok());
    /// assert_eq!(ExternalUrl::parse(" file:///tmp/x"), Err(ExternalUrlError::SurroundingWhitespace));
    /// assert_eq!(ExternalUrl::parse("file:///tmp/x"), Err(ExternalUrlError::UnsupportedScheme));
    /// ```
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

    /// Borrows the exact accepted source string.
    ///
    /// No normalization, redaction, or percent-decoding is performed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::ExternalUrl;
    /// let url = ExternalUrl::parse("HTTPS://EXAMPLE.COM/a%20b?q=1#x")?;
    /// assert_eq!(url.as_str(), "HTTPS://EXAMPLE.COM/a%20b?q=1#x");
    /// # Ok::<(), ailloli_ui_runtime::app::ExternalUrlError>(())
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Implements the `TryFrom<&str>` contract for `ExternalUrl`.
impl TryFrom<&str> for ExternalUrl {
    /// Error type produced by this conversion.
    type Error = ExternalUrlError;

    /// Validates and performs the requested conversion.
    ///
    /// # Errors
    ///
    /// Propagates the validation errors documented by [`ExternalUrl::parse`].
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

/// Implements the `TryFrom<String>` contract for `ExternalUrl`.
impl TryFrom<String> for ExternalUrl {
    /// Error type produced by this conversion.
    type Error = ExternalUrlError;

    /// Validates and performs the requested conversion.
    ///
    /// # Errors
    ///
    /// Propagates the validation errors documented by [`ExternalUrl::parse`].
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

/// Why an external URL was rejected. Error messages intentionally omit input.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::ExternalUrlError;
/// assert_eq!(ExternalUrlError::Empty.to_string(), "external URL is empty");
/// assert!(!ExternalUrlError::Malformed.to_string().contains("secret"));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalUrlError {
    /// The source contains zero bytes.
    Empty,
    /// [`str::trim`] would remove leading or trailing Unicode whitespace.
    SurroundingWhitespace,
    /// `url::Url` could not parse the source as an absolute URL.
    Malformed,
    /// The parsed scheme is neither HTTP nor HTTPS.
    UnsupportedScheme,
    /// The parsed HTTP(S) URL has no host.
    MissingHost,
}

/// Implements the fmt::Display contract for ExternalUrlError.
impl fmt::Display for ExternalUrlError {
    /// Formats the value for human-readable diagnostics.
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

/// Implements the std::error::Error contract for ExternalUrlError.
impl std::error::Error for ExternalUrlError {}

/// A non-fatal failure while handing a validated URL to the host system.
///
/// Errors deliberately carry no URL, process command, or platform detail so
/// UI diagnostics cannot accidentally expose credentials or query secrets.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::OpenUrlError;
/// assert_eq!(OpenUrlError::Unavailable.to_string(), "system URL opener is unavailable");
/// assert_eq!(OpenUrlError::LaunchFailed.to_string(), "system URL opener failed");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenUrlError {
    /// No suitable host URL-opening capability is installed or enabled.
    Unavailable,
    /// A configured opener failed to accept or launch the URL.
    LaunchFailed,
}

/// Implements the fmt::Display contract for OpenUrlError.
impl fmt::Display for OpenUrlError {
    /// Formats the value for human-readable diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("system URL opener is unavailable"),
            Self::LaunchFailed => f.write_str("system URL opener failed"),
        }
    }
}

/// Implements the std::error::Error contract for OpenUrlError.
impl std::error::Error for OpenUrlError {}

/// Provider-neutral capability used by widgets to open validated URLs.
///
/// Implementations should return promptly, must not shell-interpolate the URL,
/// and should avoid logging its potentially sensitive exact source.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{ExternalUrl, ExternalUrlOpener, MemoryExternalUrlOpener};
/// let opener: &dyn ExternalUrlOpener = &MemoryExternalUrlOpener::new();
/// let url = ExternalUrl::parse("https://example.com")?;
/// opener.open(&url)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub trait ExternalUrlOpener {
    /// Hands one already validated URL to the host.
    ///
    /// A successful return means the host accepted the request, not that a
    /// browser loaded the resource.
    ///
    /// # Errors
    ///
    /// Returns [`OpenUrlError::Unavailable`] when no host capability can accept
    /// the URL, or [`OpenUrlError::LaunchFailed`] when an available capability
    /// fails to launch it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{ExternalUrl, ExternalUrlOpener, MemoryExternalUrlOpener};
    /// let opener = MemoryExternalUrlOpener::new();
    /// let url = ExternalUrl::parse("https://example.com")?;
    /// assert_eq!(opener.open(&url), Ok(()));
    /// assert_eq!(opener.opened_urls(), ["https://example.com"]);
    /// # Ok::<(), ailloli_ui_runtime::app::ExternalUrlError>(())
    /// ```
    fn open(&self, url: &ExternalUrl) -> Result<(), OpenUrlError>;
}

/// Deterministic opener for headless runtimes and tests.
///
/// Clones share recorded URLs and the one-shot error slot through `Rc<RefCell<_>>`.
/// The type is neither `Send` nor `Sync`. Successful opens append exact URL
/// strings without a capacity bound; tests should drain them when appropriate.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{ExternalUrl, ExternalUrlOpener, MemoryExternalUrlOpener};
/// let opener = MemoryExternalUrlOpener::new();
/// let alias = opener.clone();
/// alias.open(&ExternalUrl::parse("https://example.com")?)?;
/// assert_eq!(opener.opened_urls(), ["https://example.com"]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Default)]
pub struct MemoryExternalUrlOpener {
    /// Shared unbounded successful-open log.
    opened: Rc<RefCell<Vec<String>>>,
    /// Shared error consumed by the next open attempt only.
    next_error: Rc<RefCell<Option<OpenUrlError>>>,
}

/// Provides the operations defined for MemoryExternalUrlOpener.
impl MemoryExternalUrlOpener {
    /// Creates an opener with an empty log and no scheduled failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::MemoryExternalUrlOpener;
    /// let opener = MemoryExternalUrlOpener::new();
    /// assert!(opener.opened_urls().is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Clones and returns all successful URL sources in call order.
    ///
    /// This is O(total stored URL bytes) and does not clear the log.
    ///
    /// # Panics
    ///
    /// Panics on a conflicting borrow through a reentrant alias.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{ExternalUrl, ExternalUrlOpener, MemoryExternalUrlOpener};
    /// let opener = MemoryExternalUrlOpener::new();
    /// opener.open(&ExternalUrl::parse("https://example.com/a")?)?;
    /// assert_eq!(opener.opened_urls(), ["https://example.com/a"]);
    /// assert_eq!(opener.opened_urls().len(), 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn opened_urls(&self) -> Vec<String> {
        self.opened.borrow().clone()
    }

    /// Drains and returns all successful URL sources in call order.
    ///
    /// A second call returns an empty vector until another successful open.
    ///
    /// # Panics
    ///
    /// Panics on a conflicting borrow through a reentrant alias.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{ExternalUrl, ExternalUrlOpener, MemoryExternalUrlOpener};
    /// let opener = MemoryExternalUrlOpener::new();
    /// opener.open(&ExternalUrl::parse("https://example.com")?)?;
    /// assert_eq!(opener.take_opened_urls(), ["https://example.com"]);
    /// assert!(opener.take_opened_urls().is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn take_opened_urls(&self) -> Vec<String> {
        std::mem::take(&mut *self.opened.borrow_mut())
    }

    /// Makes the next open attempt return `error` without recording its URL.
    ///
    /// Calling this again before an attempt replaces the pending error. Clones
    /// observe and consume the same one-shot slot.
    ///
    /// # Panics
    ///
    /// Panics on a conflicting borrow through a reentrant alias.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{ExternalUrl, ExternalUrlOpener, MemoryExternalUrlOpener, OpenUrlError};
    /// let opener = MemoryExternalUrlOpener::new();
    /// let url = ExternalUrl::parse("https://example.com")?;
    /// opener.fail_next(OpenUrlError::Unavailable);
    /// assert_eq!(opener.open(&url), Err(OpenUrlError::Unavailable));
    /// assert_eq!(opener.open(&url), Ok(()));
    /// # Ok::<(), ailloli_ui_runtime::app::ExternalUrlError>(())
    /// ```
    pub fn fail_next(&self, error: OpenUrlError) {
        *self.next_error.borrow_mut() = Some(error);
    }
}

/// Implements the ExternalUrlOpener contract for MemoryExternalUrlOpener.
impl ExternalUrlOpener for MemoryExternalUrlOpener {
    /// Attempts to open the validated external URL.
    ///
    /// # Errors
    ///
    /// Returns and consumes the error installed by [`Self::fail_next`], if any.
    ///
    /// # Panics
    ///
    /// Panics on a conflicting reentrant borrow of the opener's local state.
    fn open(&self, url: &ExternalUrl) -> Result<(), OpenUrlError> {
        if let Some(error) = self.next_error.borrow_mut().take() {
            return Err(error);
        }
        self.opened.borrow_mut().push(url.as_str().to_owned());
        Ok(())
    }
}

#[cfg(test)]
/// Tests implementation details.
mod tests {
    use super::*;

    #[test]
    /// Verifies that accepts http https and preserves exact source.
    fn accepts_http_https_and_preserves_exact_source() {
        let source = "https://user:secret@example.com:8443/docs?q=token#part";
        assert_eq!(ExternalUrl::parse(source).unwrap().as_str(), source);
        assert_eq!(
            ExternalUrl::parse("http://example.com").unwrap().as_str(),
            "http://example.com"
        );
    }

    #[test]
    /// Verifies that rejects unsafe or non absolute urls.
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
    /// Verifies that validation errors do not echo sensitive input.
    fn validation_errors_do_not_echo_sensitive_input() {
        let secret = "secret-query-value";
        let error = ExternalUrl::parse(format!("javascript:{secret}")).unwrap_err();
        assert!(!error.to_string().contains(secret));
    }

    #[test]
    /// Verifies that memory opener records success and can fail once.
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

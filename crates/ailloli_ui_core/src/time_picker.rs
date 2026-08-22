//! Pure time picker values and formatting helpers.

/// A wall-clock time with minute precision.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::TimeValue;
///
/// let time = TimeValue::new(9, 30);
/// assert_eq!((time.hour, time.minute), (9, 30));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeValue {
    /// Hour in `0..=23` for values created by [`TimeValue::new`].
    pub hour: u8,
    /// Minute in `0..=59` for values created by [`TimeValue::new`].
    pub minute: u8,
}

/// Text representation used to parse or format [`TimeValue`].
///
/// Possible values are [`TimeFormat::Hour24`] and [`TimeFormat::Hour12`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{TimeFormat, TimeValue};
///
/// assert_eq!(TimeValue::new(13, 5).format(TimeFormat::Hour12), "1:05 PM");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TimeFormat {
    /// Zero-padded 24-hour `HH:MM`; this is the default.
    #[default]
    Hour24,
    /// Twelve-hour `H:MM AM` or `H:MM PM`.
    Hour12,
}

impl TimeValue {
    /// Creates a time, clamping the hour to 23 and the minute to 59.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::TimeValue;
    ///
    /// assert_eq!(TimeValue::new(25, 90), TimeValue { hour: 23, minute: 59 });
    /// ```
    pub fn new(hour: u8, minute: u8) -> Self {
        Self {
            hour: hour.min(23),
            minute: minute.min(59),
        }
    }

    /// Returns minutes elapsed since midnight in `0..=1439`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::TimeValue;
    ///
    /// assert_eq!(TimeValue::new(1, 30).total_minutes(), 90);
    /// ```
    pub fn total_minutes(self) -> u16 {
        self.hour as u16 * 60 + self.minute as u16
    }

    /// Creates a time from minutes since midnight, clamped to 23:59.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::TimeValue;
    ///
    /// assert_eq!(TimeValue::from_total_minutes(75), TimeValue::new(1, 15));
    /// ```
    pub fn from_total_minutes(minutes: u16) -> Self {
        let minutes = minutes.min(23 * 60 + 59);
        Self::new((minutes / 60) as u8, (minutes % 60) as u8)
    }

    /// Formats this value using the selected 12- or 24-hour representation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{TimeFormat, TimeValue};
    ///
    /// assert_eq!(TimeValue::new(0, 5).format(TimeFormat::Hour24), "00:05");
    /// ```
    pub fn format(self, format: TimeFormat) -> String {
        match format {
            TimeFormat::Hour24 => format!("{:02}:{:02}", self.hour, self.minute),
            TimeFormat::Hour12 => {
                let suffix = if self.hour < 12 { "AM" } else { "PM" };
                let hour = match self.hour % 12 {
                    0 => 12,
                    h => h,
                };
                format!("{hour}:{:02} {suffix}", self.minute)
            }
        }
    }
}

/// Parses a time in the exact representation selected by `format`.
///
/// The 24-hour form requires `hour:minute` with values in `0..=23` and
/// `0..=59`. The 12-hour form requires a space plus case-insensitive `AM` or
/// `PM`, and an hour in `1..=12`. Numeric fields need not be zero-padded.
/// Returns `None` for malformed or out-of-range input.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{TimeFormat, TimeValue};
/// use ailloli_ui_core::time_picker::parse_time;
///
/// assert_eq!(parse_time("2:05 PM", TimeFormat::Hour12), Some(TimeValue::new(14, 5)));
/// assert_eq!(parse_time("25:00", TimeFormat::Hour24), None);
/// ```
pub fn parse_time(input: &str, format: TimeFormat) -> Option<TimeValue> {
    match format {
        TimeFormat::Hour24 => {
            let mut parts = input.split(':');
            let hour = parts.next()?.parse::<u8>().ok()?;
            let minute = parts.next()?.parse::<u8>().ok()?;
            if parts.next().is_some() || hour > 23 || minute > 59 {
                return None;
            }
            Some(TimeValue { hour, minute })
        }
        TimeFormat::Hour12 => {
            let trimmed = input.trim();
            let (time, suffix) = trimmed.rsplit_once(' ')?;
            let suffix = suffix.to_ascii_uppercase();
            if suffix != "AM" && suffix != "PM" {
                return None;
            }
            let mut parts = time.split(':');
            let hour12 = parts.next()?.parse::<u8>().ok()?;
            let minute = parts.next()?.parse::<u8>().ok()?;
            if parts.next().is_some() || !(1..=12).contains(&hour12) || minute > 59 {
                return None;
            }
            let hour = match (hour12, suffix.as_str()) {
                (12, "AM") => 0,
                (12, "PM") => 12,
                (h, "PM") => h + 12,
                (h, _) => h,
            };
            Some(TimeValue { hour, minute })
        }
    }
}

/// Clamps a minute step to the supported inclusive range `1..=60`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::time_picker::sanitize_step_minutes;
///
/// assert_eq!(sanitize_step_minutes(0), 1);
/// assert_eq!(sanitize_step_minutes(90), 60);
/// ```
pub fn sanitize_step_minutes(step: u8) -> u8 {
    step.clamp(1, 60)
}

/// Rounds a time to the nearest sanitized minute step.
///
/// Half-step ties round upward. The final value is clamped to 23:59 instead of
/// wrapping into the next day.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::TimeValue;
/// use ailloli_ui_core::time_picker::snap_time;
///
/// assert_eq!(snap_time(TimeValue::new(10, 8), 5), TimeValue::new(10, 10));
/// ```
pub fn snap_time(value: TimeValue, step_minutes: u8) -> TimeValue {
    let step = sanitize_step_minutes(step_minutes) as u16;
    let minutes = value.total_minutes();
    let snapped = ((minutes + step / 2) / step) * step;
    TimeValue::from_total_minutes(snapped.min(23 * 60 + 59))
}

/// Moves a time by `delta_minutes`, clamps it to the current day, then snaps it.
///
/// No wrap occurs at midnight. `step_minutes` is clamped with
/// [`sanitize_step_minutes`].
///
/// # Panics
///
/// Debug builds may panic if adding an extreme `i16` delta to the current
/// minute count overflows `i16`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::TimeValue;
/// use ailloli_ui_core::time_picker::nudge_time;
///
/// assert_eq!(nudge_time(TimeValue::new(10, 0), 5, 5), TimeValue::new(10, 5));
/// ```
pub fn nudge_time(value: TimeValue, delta_minutes: i16, step_minutes: u8) -> TimeValue {
    let current = value.total_minutes() as i16;
    let next = (current + delta_minutes).clamp(0, 23 * 60 + 59);
    snap_time(TimeValue::from_total_minutes(next as u16), step_minutes)
}

#[cfg(test)]
mod tests {
    //! Covers both text formats and nearest-step snapping.

    use super::*;

    #[test]
    fn time_formats_and_parses() {
        let value = TimeValue::new(14, 5);
        assert_eq!(value.format(TimeFormat::Hour24), "14:05");
        assert_eq!(value.format(TimeFormat::Hour12), "2:05 PM");
        assert_eq!(parse_time("2:05 PM", TimeFormat::Hour12), Some(value));
        assert_eq!(parse_time("14:05", TimeFormat::Hour24), Some(value));
    }

    #[test]
    fn snap_uses_nearest_step() {
        assert_eq!(snap_time(TimeValue::new(10, 7), 5), TimeValue::new(10, 5));
        assert_eq!(snap_time(TimeValue::new(10, 8), 5), TimeValue::new(10, 10));
    }
}

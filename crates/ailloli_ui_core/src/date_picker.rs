//! Pure date picker values and calendar helpers.

/// A date in the proleptic Gregorian calendar.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::DateValue;
///
/// let date = DateValue::new(2024, 2, 31);
/// assert_eq!(date.format_yyyy_mm_dd(), "2024-02-29");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateValue {
    /// Signed calendar year.
    pub year: i32,
    /// One-based month in `1..=12` for values created by [`DateValue::new`].
    pub month: u8,
    /// One-based day, bounded by the selected month for validated values.
    pub day: u8,
}

/// A year and month in the proleptic Gregorian calendar.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::MonthValue;
///
/// assert_eq!(MonthValue::new(2026, 13).month, 12);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonthValue {
    /// Signed calendar year.
    pub year: i32,
    /// One-based month in `1..=12` for values created by [`MonthValue::new`].
    pub month: u8,
}

/// First column of a generated calendar week.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::WeekStart;
///
/// assert_eq!(WeekStart::default(), WeekStart::Monday);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WeekStart {
    /// Weeks begin on Sunday.
    Sunday,
    /// Weeks begin on Monday; this is the default.
    #[default]
    Monday,
}

/// One cell in the fixed six-week grid returned by [`month_grid`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::date_picker::CalendarDay;
/// use ailloli_ui_core::DateValue;
///
/// let day = CalendarDay { date: DateValue::new(2026, 5, 1), in_month: true };
/// assert!(day.in_month);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarDay {
    /// Date represented by this cell.
    pub date: DateValue,
    /// `true` when `date` belongs to the requested month; `false` for padding.
    pub in_month: bool,
}

impl DateValue {
    /// Creates a date, clamping the month and day to a valid calendar value.
    ///
    /// Months clamp to `1..=12`; days then clamp to the valid range for that
    /// month and year.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::DateValue;
    /// assert_eq!(DateValue::new(2024, 2, 31), DateValue { year: 2024, month: 2, day: 29 });
    /// ```
    pub fn new(year: i32, month: u8, day: u8) -> Self {
        let month = month.clamp(1, 12);
        let day = day.clamp(1, days_in_month(year, month));
        Self { year, month, day }
    }

    /// Returns this date's year and month.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{DateValue, MonthValue};
    /// assert_eq!(DateValue::new(2024, 7, 3).month_value(), MonthValue::new(2024, 7));
    /// ```
    pub fn month_value(self) -> MonthValue {
        MonthValue::new(self.year, self.month)
    }

    /// Formats the date as a zero-padded `YYYY-MM-DD` string.
    ///
    /// Years wider than four digits are not truncated.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::DateValue;
    /// assert_eq!(DateValue::new(2024, 7, 3).format_yyyy_mm_dd(), "2024-07-03");
    /// ```
    pub fn format_yyyy_mm_dd(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl MonthValue {
    /// Creates a month, clamping `month` to `1..=12`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::MonthValue;
    /// assert_eq!(MonthValue::new(2024, 20), MonthValue { year: 2024, month: 12 });
    /// ```
    pub fn new(year: i32, month: u8) -> Self {
        Self {
            year,
            month: month.clamp(1, 12),
        }
    }
}

/// Parses an exact decimal `year-month-day` date with a valid month and day.
///
/// Returns `None` for missing or extra components, numeric parse failures, or
/// an out-of-range month/day. Surrounding whitespace and `+` signs follow the
/// underlying integer parsers; callers needing a stricter wire format should
/// validate the original string separately.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::date_picker::parse_yyyy_mm_dd;
/// use ailloli_ui_core::DateValue;
///
/// assert_eq!(parse_yyyy_mm_dd("2024-02-29"), Some(DateValue::new(2024, 2, 29)));
/// assert_eq!(parse_yyyy_mm_dd("2023-02-29"), None);
/// ```
pub fn parse_yyyy_mm_dd(input: &str) -> Option<DateValue> {
    let mut parts = input.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u8>().ok()?;
    let day = parts.next()?.parse::<u8>().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }
    if !(1..=days_in_month(year, month)).contains(&day) {
        return None;
    }
    Some(DateValue { year, month, day })
}

/// Returns whether `year` is a leap year in the proleptic Gregorian calendar.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::date_picker::is_leap_year;
///
/// assert!(is_leap_year(2000));
/// assert!(!is_leap_year(2100));
/// ```
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Returns the number of days in `month` for `year`.
///
/// Invalid month values are clamped to `1..=12` before evaluation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::date_picker::days_in_month;
///
/// assert_eq!(days_in_month(2024, 2), 29);
/// assert_eq!(days_in_month(2026, 13), 31);
/// ```
pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month.clamp(1, 12) {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 30,
    }
}

/// Clamps a date to optional inclusive bounds.
///
/// The lower bound is applied before the upper bound. If `min > max`, the
/// result is therefore `max`; callers should normally provide ordered bounds.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::date_picker::clamp_date;
/// use ailloli_ui_core::DateValue;
///
/// let min = DateValue::new(2026, 1, 10);
/// assert_eq!(clamp_date(DateValue::new(2026, 1, 1), Some(min), None), min);
/// ```
pub fn clamp_date(value: DateValue, min: Option<DateValue>, max: Option<DateValue>) -> DateValue {
    let mut value = value;
    if let Some(min) = min {
        value = value.max(min);
    }
    if let Some(max) = max {
        value = value.min(max);
    }
    value
}

/// Returns whether a date falls within both optional inclusive bounds.
///
/// With no bounds every value is enabled. Inverted bounds disable every date.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::date_picker::is_date_enabled;
/// use ailloli_ui_core::DateValue;
///
/// let date = DateValue::new(2026, 5, 4);
/// assert!(is_date_enabled(date, None, None));
/// assert!(!is_date_enabled(date, Some(DateValue::new(2026, 5, 5)), None));
/// ```
pub fn is_date_enabled(value: DateValue, min: Option<DateValue>, max: Option<DateValue>) -> bool {
    min.is_none_or(|min| value >= min) && max.is_none_or(|max| value <= max)
}

/// Adds a signed number of calendar months using Euclidean year rollover.
///
/// # Panics
///
/// Debug builds may panic if the intermediate `year * 12 + delta` overflows
/// `i32`; use ordinary application-scale years and deltas.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::date_picker::add_months;
/// use ailloli_ui_core::MonthValue;
///
/// assert_eq!(add_months(MonthValue::new(2026, 1), -2), MonthValue::new(2025, 11));
/// ```
pub fn add_months(month: MonthValue, delta: i32) -> MonthValue {
    let zero_based = month.year * 12 + (month.month as i32 - 1) + delta;
    let year = zero_based.div_euclid(12);
    let month = zero_based.rem_euclid(12) as u8 + 1;
    MonthValue { year, month }
}

/// Moves a date by a signed number of calendar days.
///
/// Runtime is proportional to the number of crossed months.
///
/// # Panics
///
/// Debug builds may panic if adding `delta` to the day or crossing an extreme
/// `i32` year overflows.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::date_picker::next_day;
/// use ailloli_ui_core::DateValue;
///
/// assert_eq!(next_day(DateValue::new(2024, 2, 28), 1), DateValue::new(2024, 2, 29));
/// ```
pub fn next_day(date: DateValue, delta: i32) -> DateValue {
    let mut year = date.year;
    let mut month = date.month;
    let mut day = date.day as i32 + delta;
    loop {
        let dim = days_in_month(year, month) as i32;
        if day < 1 {
            let prev = add_months(MonthValue::new(year, month), -1);
            year = prev.year;
            month = prev.month;
            day += days_in_month(year, month) as i32;
        } else if day > dim {
            day -= dim;
            let next = add_months(MonthValue::new(year, month), 1);
            year = next.year;
            month = next.month;
        } else {
            return DateValue::new(year, month, day as u8);
        }
    }
}

/// Returns the zero-based weekday column for `date` and `week_start`.
///
/// The result is always in `0..=6`; `0` denotes the requested first weekday.
///
/// # Panics
///
/// Debug builds may panic for extreme `i32` years whose calendar arithmetic
/// overflows.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::date_picker::weekday_index;
/// use ailloli_ui_core::{DateValue, WeekStart};
///
/// assert_eq!(weekday_index(DateValue::new(2026, 5, 4), WeekStart::Monday), 0);
/// ```
pub fn weekday_index(date: DateValue, week_start: WeekStart) -> u8 {
    let mut y = date.year;
    let mut m = date.month as i32;
    let d = date.day as i32;
    if m < 3 {
        m += 12;
        y -= 1;
    }
    let k = y.rem_euclid(100);
    let j = y.div_euclid(100);
    let h = (d + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 + 5 * j).rem_euclid(7);
    let sunday_based = ((h + 6) % 7) as u8;
    match week_start {
        WeekStart::Sunday => sunday_based,
        WeekStart::Monday => (sunday_based + 6) % 7,
    }
}

/// Generates a fixed 42-cell, six-week calendar grid around `month`.
///
/// Leading and trailing cells belong to adjacent months and have
/// [`CalendarDay::in_month`] set to `false`.
///
/// # Panics
///
/// Debug builds may panic for extreme `i32` years when the delegated weekday
/// or adjacent-day arithmetic overflows.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::date_picker::month_grid;
/// use ailloli_ui_core::{MonthValue, WeekStart};
///
/// let grid = month_grid(MonthValue::new(2026, 5), WeekStart::Monday);
/// assert_eq!(grid.len(), 42);
/// assert!(grid.iter().any(|day| !day.in_month));
/// ```
pub fn month_grid(month: MonthValue, week_start: WeekStart) -> [CalendarDay; 42] {
    let first = DateValue::new(month.year, month.month, 1);
    let start = next_day(first, -(weekday_index(first, week_start) as i32));
    std::array::from_fn(|idx| {
        let date = next_day(start, idx as i32);
        CalendarDay {
            date,
            in_month: date.year == month.year && date.month == month.month,
        }
    })
}

#[cfg(test)]
mod tests {
    //! Covers leap years, six-week grids, and month rollover in both directions.

    use super::*;

    #[test]
    fn leap_year_and_days_are_stable() {
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2026));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
    }

    #[test]
    fn month_grid_is_six_weeks_with_monday_start() {
        let grid = month_grid(MonthValue::new(2026, 5), WeekStart::Monday);
        assert_eq!(grid.len(), 42);
        assert_eq!(grid[0].date, DateValue::new(2026, 4, 27));
        assert_eq!(grid[4].date, DateValue::new(2026, 5, 1));
    }

    #[test]
    fn add_months_crosses_years() {
        assert_eq!(
            add_months(MonthValue::new(2026, 1), -2),
            MonthValue::new(2025, 11)
        );
        assert_eq!(
            add_months(MonthValue::new(2026, 12), 2),
            MonthValue::new(2027, 2)
        );
    }
}

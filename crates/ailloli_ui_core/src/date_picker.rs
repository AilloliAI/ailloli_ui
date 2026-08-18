//! Pure date picker values and calendar helpers.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateValue {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonthValue {
    pub year: i32,
    pub month: u8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WeekStart {
    Sunday,
    #[default]
    Monday,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarDay {
    pub date: DateValue,
    pub in_month: bool,
}

impl DateValue {
    pub fn new(year: i32, month: u8, day: u8) -> Self {
        let month = month.clamp(1, 12);
        let day = day.clamp(1, days_in_month(year, month));
        Self { year, month, day }
    }

    pub fn month_value(self) -> MonthValue {
        MonthValue::new(self.year, self.month)
    }

    pub fn format_yyyy_mm_dd(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl MonthValue {
    pub fn new(year: i32, month: u8) -> Self {
        Self {
            year,
            month: month.clamp(1, 12),
        }
    }
}

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

pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month.clamp(1, 12) {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 30,
    }
}

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

pub fn is_date_enabled(value: DateValue, min: Option<DateValue>, max: Option<DateValue>) -> bool {
    min.is_none_or(|min| value >= min) && max.is_none_or(|max| value <= max)
}

pub fn add_months(month: MonthValue, delta: i32) -> MonthValue {
    let zero_based = month.year * 12 + (month.month as i32 - 1) + delta;
    let year = zero_based.div_euclid(12);
    let month = zero_based.rem_euclid(12) as u8 + 1;
    MonthValue { year, month }
}

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

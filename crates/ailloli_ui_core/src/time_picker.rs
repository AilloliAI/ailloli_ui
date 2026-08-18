//! Pure time picker values and formatting helpers.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeValue {
    pub hour: u8,
    pub minute: u8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TimeFormat {
    #[default]
    Hour24,
    Hour12,
}

impl TimeValue {
    pub fn new(hour: u8, minute: u8) -> Self {
        Self {
            hour: hour.min(23),
            minute: minute.min(59),
        }
    }

    pub fn total_minutes(self) -> u16 {
        self.hour as u16 * 60 + self.minute as u16
    }

    pub fn from_total_minutes(minutes: u16) -> Self {
        let minutes = minutes.min(23 * 60 + 59);
        Self::new((minutes / 60) as u8, (minutes % 60) as u8)
    }

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

pub fn sanitize_step_minutes(step: u8) -> u8 {
    step.clamp(1, 60)
}

pub fn snap_time(value: TimeValue, step_minutes: u8) -> TimeValue {
    let step = sanitize_step_minutes(step_minutes) as u16;
    let minutes = value.total_minutes();
    let snapped = ((minutes + step / 2) / step) * step;
    TimeValue::from_total_minutes(snapped.min(23 * 60 + 59))
}

pub fn nudge_time(value: TimeValue, delta_minutes: i16, step_minutes: u8) -> TimeValue {
    let current = value.total_minutes() as i16;
    let next = (current + delta_minutes).clamp(0, 23 * 60 + 59);
    snap_time(TimeValue::from_total_minutes(next as u16), step_minutes)
}

#[cfg(test)]
mod tests {
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

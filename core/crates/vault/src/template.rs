//! Templates and dates the way Obsidian writes them: `{{title}}`,
//! `{{date}}`, `{{time}}` and `{{date:YYYY-MM-DD}}` in templates, and
//! Moment.js formats (`YYYY-MM-DD`, `gggg-[W]ww`, `YYYY/MM/DD`) for daily
//! note names.

/// A local date and time, from the app (the core does not know the time
/// zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

impl Moment {
    fn days(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }

    /// 0 for Sunday … 6 for Saturday.
    fn weekday(&self) -> usize {
        (self.days() + 4).rem_euclid(7) as usize
    }

    fn day_of_year(&self) -> i64 {
        self.days() - days_from_civil(self.year, 1, 1) + 1
    }

    /// The ISO week-numbering year and week.
    fn iso_week(&self) -> (i32, i64) {
        let monday_based = (self.weekday() + 6) % 7;
        let thursday = self.days() - monday_based as i64 + 3;
        // The Thursday's year is the week's year.
        let mut year = self.year + 1;
        while days_from_civil(year, 1, 1) > thursday {
            year -= 1;
        }
        (year, (thursday - days_from_civil(year, 1, 1)) / 7 + 1)
    }
}

/// Formats a date with Moment.js tokens; text in `[brackets]` is kept.
pub fn format(pattern: &str, at: &Moment) -> String {
    const TOKENS: [&str; 30] = [
        "YYYY", "GGGG", "gggg", "MMMM", "dddd", "DDDD", "MMM", "ddd", "DDD", "YY", "MM", "DD", "Do", "dd",
        "HH", "hh", "mm", "ss", "WW", "ww", "M", "D", "d", "H", "h", "m", "s", "W", "w", "A",
    ];
    let (week_year, week) = at.iso_week();
    let hour12 = match at.hour % 12 {
        0 => 12,
        h => h,
    };
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(c) = rest.chars().next() {
        if c == '[' {
            let end = rest.find(']').unwrap_or(rest.len());
            out.push_str(&rest[1..end]);
            rest = rest.get(end + 1..).unwrap_or("");
            continue;
        }
        let Some(token) = TOKENS.iter().find(|t| rest.starts_with(**t)) else {
            out.push(c);
            rest = &rest[c.len_utf8()..];
            continue;
        };
        let text = match *token {
            "YYYY" => format!("{:04}", at.year),
            "YY" => format!("{:02}", at.year.rem_euclid(100)),
            "GGGG" | "gggg" => format!("{week_year:04}"),
            "MMMM" => MONTHS[(at.month.clamp(1, 12) - 1) as usize].to_string(),
            "MMM" => MONTHS[(at.month.clamp(1, 12) - 1) as usize][..3].to_string(),
            "MM" => format!("{:02}", at.month),
            "M" => at.month.to_string(),
            "DDDD" => format!("{:03}", at.day_of_year()),
            "DDD" => at.day_of_year().to_string(),
            "DD" => format!("{:02}", at.day),
            "D" => at.day.to_string(),
            "Do" => format!("{}{}", at.day, ordinal(at.day)),
            "dddd" => WEEKDAYS[at.weekday()].to_string(),
            "ddd" => WEEKDAYS[at.weekday()][..3].to_string(),
            "dd" => WEEKDAYS[at.weekday()][..2].to_string(),
            "d" => at.weekday().to_string(),
            "HH" => format!("{:02}", at.hour),
            "H" => at.hour.to_string(),
            "hh" => format!("{hour12:02}"),
            "h" => hour12.to_string(),
            "mm" => format!("{:02}", at.minute),
            "m" => at.minute.to_string(),
            "ss" => format!("{:02}", at.second),
            "s" => at.second.to_string(),
            "WW" | "ww" => format!("{week:02}"),
            "W" | "w" => week.to_string(),
            _ => (if at.hour < 12 { "AM" } else { "PM" }).to_string(),
        };
        out.push_str(&text);
        rest = &rest[token.len()..];
    }
    out
}

fn ordinal(day: u32) -> &'static str {
    match (day % 10, day % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

/// A template filled in: `{{title}}`, `{{date}}` and `{{time}}` (in the
/// vault's formats), `{{date:FORMAT}}` and `{{time:FORMAT}}`. Anything else
/// in braces stays.
pub fn fill(template: &str, title: &str, at: &Moment, date_format: &str, time_format: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inside = after[..end].trim();
        let (name, pattern) = match inside.split_once(':') {
            Some((name, pattern)) => (name.trim(), Some(pattern.trim())),
            None => (inside, None),
        };
        match (name.to_ascii_lowercase().as_str(), pattern) {
            ("title", _) => out.push_str(title),
            ("date", pattern) => out.push_str(&format(pattern.unwrap_or(date_format), at)),
            ("time", pattern) => out.push_str(&format(pattern.unwrap_or(time_format), at)),
            _ => out.push_str(&rest[start..start + 2 + end + 2]),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: Moment = Moment { year: 2026, month: 10, day: 12, hour: 15, minute: 4, second: 5 };

    #[test]
    fn formats_like_moment() {
        assert_eq!(format("YYYY-MM-DD", &AT), "2026-10-12");
        assert_eq!(format("YYYY/MM/[Notes] D, dddd", &AT), "2026/10/Notes 12, Monday");
        assert_eq!(format("MMMM Do, YY h:mm A", &AT), "October 12th, 26 3:04 PM");
        assert_eq!(format("gggg-[W]ww", &AT), "2026-W42");
        assert_eq!(format("DDDD", &AT), "285");
        let new_year = Moment { year: 2027, month: 1, day: 1, hour: 0, minute: 0, second: 0 };
        assert_eq!(format("GGGG-[W]WW ddd hh A", &new_year), "2026-W53 Fri 12 AM");
        assert_eq!(format("年YYYY", &AT), "年2026");
    }

    #[test]
    fn fills_templates() {
        let template = "# {{title}}\nCreated {{date}} {{time}} ({{ date:dddd }})\n{{unknown}} {{";
        assert_eq!(
            fill(template, "Reading", &AT, "YYYY-MM-DD", "HH:mm"),
            "# Reading\nCreated 2026-10-12 15:04 (Monday)\n{{unknown}} {{"
        );
    }
}

//! Calendar helpers that behave like the spreadsheet functions the workbook used
//! (`EDATE`, `DATE` overflow) plus Russian date parsing and formatting.

use chrono::{Datelike, Duration, Months, NaiveDate};

pub type Date = NaiveDate;

pub fn today() -> Date {
    chrono::Local::now().date_naive()
}

/// `EDATE`: shift by whole months, clamping the day to the end of the month.
pub fn edate(d: Date, months: i32) -> Date {
    let shifted = if months >= 0 {
        d.checked_add_months(Months::new(months as u32))
    } else {
        d.checked_sub_months(Months::new(months.unsigned_abs()))
    };
    shifted.unwrap_or(d)
}

/// `DATE(y, m, d)` for a valid month: a day past the end of the month rolls over
/// (29 Feb of a common year is 1 Mar).
pub fn excel_date(y: i32, m: u32, d: u32) -> Option<Date> {
    NaiveDate::from_ymd_opt(y, m, 1).map(|first| first + Duration::days(d as i64 - 1))
}

/// Spreadsheet serial number → date (1900 system, so serial 25569 is 1970-01-01).
pub fn from_serial(serial: f64) -> Option<Date> {
    if !serial.is_finite() || serial < 1.0 {
        return None;
    }
    NaiveDate::from_ymd_opt(1899, 12, 30).map(|base| base + Duration::days(serial.floor() as i64))
}

pub fn iso(d: Date) -> String {
    d.format("%Y-%m-%d").to_string()
}

pub fn parse_iso(s: &str) -> Option<Date> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

/// `28.09.2022`
pub fn fmt(d: Date) -> String {
    d.format("%d.%m.%Y").to_string()
}

/// `28.09.22`
pub fn fmt_short(d: Date) -> String {
    d.format("%d.%m.%y").to_string()
}

pub fn fmt_opt(d: Option<Date>) -> String {
    d.map(fmt).unwrap_or_default()
}

/// Accepts `28.09.2022`, `28.09.22`, `28/09/2022`, `28-09-2022`, `28092022`, `2022-09-28`.
/// Valid range matches the workbook: 1950..=2100.
pub fn parse_ru(s: &str) -> Option<Date> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let digits_only: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    let (d, m, y): (u32, u32, i32) = if s.chars().all(|c| c.is_ascii_digit()) {
        match s.len() {
            8 => (digits_only[0..2].parse().ok()?, digits_only[2..4].parse().ok()?, digits_only[4..8].parse().ok()?),
            6 => (digits_only[0..2].parse().ok()?, digits_only[2..4].parse().ok()?, expand_year(digits_only[4..6].parse().ok()?)),
            _ => return None,
        }
    } else {
        let parts: Vec<&str> = s.split(['.', '/', '-', ' ', ',']).filter(|p| !p.is_empty()).collect();
        if parts.len() != 3 {
            return None;
        }
        if parts[0].len() == 4 {
            // ISO order: yyyy-mm-dd
            (parts[2].parse().ok()?, parts[1].parse().ok()?, parts[0].parse().ok()?)
        } else {
            let y: i32 = parts[2].parse().ok()?;
            let y = if parts[2].len() <= 2 { expand_year(y) } else { y };
            (parts[0].parse().ok()?, parts[1].parse().ok()?, y)
        }
    };
    let date = NaiveDate::from_ymd_opt(y, m, d)?;
    in_range(date).then_some(date)
}

fn expand_year(y: i32) -> i32 {
    if y < 70 {
        2000 + y
    } else {
        1900 + y
    }
}

pub fn in_range(d: Date) -> bool {
    (1950..=2100).contains(&d.year())
}

pub const MONTHS: [&str; 12] =
    ["Январь", "Февраль", "Март", "Апрель", "Май", "Июнь", "Июль", "Август", "Сентябрь", "Октябрь", "Ноябрь", "Декабрь"];

pub const WEEKDAYS: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];

pub fn days_in_month(y: i32, m: u32) -> u32 {
    let first = NaiveDate::from_ymd_opt(y, m, 1).expect("valid month");
    let next = if m == 12 { NaiveDate::from_ymd_opt(y + 1, 1, 1) } else { NaiveDate::from_ymd_opt(y, m + 1, 1) };
    next.map(|n| (n - first).num_days() as u32).unwrap_or(31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> Date {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn edate_clamps_like_excel() {
        assert_eq!(edate(d(2022, 1, 31), 1), d(2022, 2, 28));
        assert_eq!(edate(d(2024, 2, 29), 12), d(2025, 2, 28));
        assert_eq!(edate(d(2022, 9, 28), 6), d(2023, 3, 28));
        assert_eq!(edate(d(2022, 9, 28), -12), d(2021, 9, 28));
    }

    #[test]
    fn date_overflow_rolls_over() {
        assert_eq!(excel_date(2025, 2, 29, ), Some(d(2025, 3, 1)));
        assert_eq!(excel_date(2024, 2, 29), Some(d(2024, 2, 29)));
    }

    #[test]
    fn serial_numbers() {
        assert_eq!(from_serial(25569.0), Some(d(1970, 1, 1)));
        assert_eq!(from_serial(44835.5), Some(d(2022, 10, 1)));
    }

    #[test]
    fn parses_russian_dates() {
        assert_eq!(parse_ru("28.09.2022"), Some(d(2022, 9, 28)));
        assert_eq!(parse_ru("8.9.22"), Some(d(2022, 9, 8)));
        assert_eq!(parse_ru("28/09/2022"), Some(d(2022, 9, 28)));
        assert_eq!(parse_ru("28092022"), Some(d(2022, 9, 28)));
        assert_eq!(parse_ru("2022-09-28"), Some(d(2022, 9, 28)));
        assert_eq!(parse_ru("31.02.2022"), None);
        assert_eq!(parse_ru("28.09.1900"), None);
        assert_eq!(parse_ru("abc"), None);
        assert_eq!(parse_ru(""), None);
    }

    #[test]
    fn month_lengths() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2025, 2), 28);
        assert_eq!(days_in_month(2025, 12), 31);
    }
}

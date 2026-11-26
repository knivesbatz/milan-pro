//! Time formatting shared by the dialogs and (R44) the file logger and crash reports. Named `logger`, not `log`, so
//! `log::warn!` keeps meaning the `log` crate everywhere in the tree.
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Minutes the local clock is ahead of UTC right now (a time from another DST period is off by the DST hour).
pub fn local_offset_minutes() -> i64 {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::SYSTEMTIME;
        use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
        let (mut local, mut utc): (SYSTEMTIME, SYSTEMTIME) = (std::mem::zeroed(), std::mem::zeroed());
        GetSystemTime(&mut utc);
        GetLocalTime(&mut local);
        let minutes = |t: &SYSTEMTIME| i64::from(days_from_civil(t.wYear as i64, t.wMonth as u32, t.wDay as u32)) * 1440 + i64::from(t.wHour) * 60 + i64::from(t.wMinute);
        minutes(&local) - minutes(&utc)
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// (year, month, day) of a day count since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `t` in local time: "YYYY-MM-DD HH:MM" (`seconds`: "YYYY-MM-DD HH:MM:SS").
pub fn format_time(t: SystemTime, seconds: bool) -> String {
    format_at(t, local_offset_minutes(), seconds)
}

fn format_at(t: SystemTime, offset_minutes: i64, seconds: bool) -> String {
    let secs = t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs() as i64 + offset_minutes * 60;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, rem % 3600 / 60, rem % 60);
    if seconds { format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}") } else { format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_round_trip() {
        for (y, m, d, days) in [(1970, 1, 1, 0), (2000, 2, 29, 11016), (2026, 11, 26, 20783), (1969, 12, 31, -1)] {
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
            assert_eq!(civil_from_days(days), (y, m, d));
        }
        let t = UNIX_EPOCH + Duration::from_secs(20783 * 86400 + 13 * 3600 + 5 * 60 + 9);
        assert_eq!(format_at(t, 0, true), "2026-11-26 13:05:09");
        assert_eq!(format_at(t, 60, false), "2026-11-26 14:05");
        assert_eq!(format_at(t, -14 * 60, false), "2026-11-25 23:05");
    }
}

//! UTC timestamps without a date-time dependency.

use std::time::{SystemTime, UNIX_EPOCH};

/// `(2026-11-15T16:34:01.123Z, 2026-11-15)` for `at`, in UTC.
#[must_use]
pub fn stamp(at: SystemTime) -> (String, String) {
    let since = at.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    let date = format!("{y:04}-{m:02}-{d:02}");
    let ts = format!(
        "{date}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        since.subsec_millis()
    );
    (ts, date)
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_known_instants() {
        assert_eq!(
            stamp(UNIX_EPOCH),
            ("1970-01-01T00:00:00.000Z".into(), "1970-01-01".into())
        );
        // 2026-05-28T16:34:01.123Z
        let at = UNIX_EPOCH + Duration::from_millis(1_779_986_041_123);
        assert_eq!(stamp(at).0, "2026-05-28T16:34:01.123Z");
        // A leap day.
        let leap = UNIX_EPOCH + Duration::from_secs(1_709_164_801);
        assert_eq!(stamp(leap).1, "2024-02-29");
    }
}

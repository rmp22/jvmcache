use std::time::Duration;

pub fn format_epoch_secs(secs: u64) -> String {
    let days = secs / 86400;
    let rem = secs % 86400;
    let hours = rem / 3600;
    let rem = rem % 3600;
    let minutes = rem / 60;
    let seconds = rem % 60;

    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y, m, d, hours, minutes, seconds
    )
}

pub fn format_duration(d: Duration) -> String {
    if d.as_secs() > 0 {
        format!("{:.2}s", d.as_secs_f64())
    } else {
        format!("{}ms", d.as_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_epoch_secs_known_date() {
        assert_eq!(format_epoch_secs(0), "1970-01-01 00:00:00");
        assert_eq!(format_epoch_secs(1700000000), "2023-11-14 22:13:20");
    }
}

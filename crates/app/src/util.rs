//! Small helpers: background jobs and time formatting (local time zone).

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{LocalResult, Local, TimeZone};

/// Runs blocking work (git, file system) off the UI thread.
pub async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

/// Opens an `https://` URL in the default browser. No shell is involved, so
/// characters in the URL cannot be interpreted as commands.
pub fn open_url(url: &str) {
    if !url.starts_with("https://") || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return;
    }
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer.exe";
    let _ = std::process::Command::new(program)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Local date and time, `05/10/2026 20:41`.
pub fn date_time(t: i64) -> String {
    format_in(t, &Local, "%d/%m/%Y %H:%M")
}

pub fn relative_time(t: i64, now: i64) -> String {
    relative_in(t, now, &Local)
}

fn relative_in<Tz: TimeZone>(t: i64, now: i64, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let diff = now - t;
    match diff {
        i64::MIN..60 => "agora".into(),
        60..3600 => format!("há {} min", diff / 60),
        3600..86_400 => format!("há {} h", diff / 3600),
        86_400..2_592_000 => format!("há {} d", diff / 86_400),
        _ => format_in(t, tz, "%Y-%m-%d"),
    }
}

fn format_in<Tz: TimeZone>(t: i64, tz: &Tz, fmt: &str) -> String
where
    Tz::Offset: std::fmt::Display,
{
    match tz.timestamp_opt(t, 0) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt.format(fmt).to_string(),
        LocalResult::None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::{FixedOffset, Utc};

    #[test]
    fn formats_dates_in_a_time_zone() {
        assert_eq!(format_in(0, &Utc, "%Y%m%d-%H%M%S"), "19700101-000000");
        assert_eq!(format_in(1_792_074_612, &Utc, "%H:%M:%S"), "14:30:12");
        let brt = FixedOffset::west_opt(3 * 3600).unwrap();
        assert_eq!(format_in(1_792_074_612, &brt, "%H:%M:%S"), "11:30:12");
    }

    #[test]
    fn relative_times() {
        let now = 1_792_074_612;
        assert_eq!(relative_time(now - 10, now), "agora");
        assert_eq!(relative_time(now - 125, now), "há 2 min");
        assert_eq!(relative_time(now - 7200, now), "há 2 h");
        assert_eq!(relative_time(now - 3 * 86_400, now), "há 3 d");
        assert_eq!(relative_in(0, now, &Utc), "1970-01-01");
    }
}

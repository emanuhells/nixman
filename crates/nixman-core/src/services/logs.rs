use std::io::ErrorKind;

use tokio::process::Command;

use crate::services::types::{LogEntry, LogPriority, ServiceError};

// ── Public API ────────────────────────────────────────────────────────────────

/// Runs `journalctl -u <unit> -n <lines> --output=json --no-pager` and returns
/// the most recent `lines` log entries for that unit, ordered oldest-first.
pub async fn get(unit: &str, lines: u32) -> Result<Vec<LogEntry>, ServiceError> {
    let lines_str = lines.to_string();

    let output = Command::new("journalctl")
        .args([
            "-u",
            unit,
            "-n",
            &lines_str,
            "--output=json",
            "--no-pager",
        ])
        .output()
        .await
        .map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                ServiceError::CommandNotFound
            } else {
                ServiceError::CommandFailed {
                    exit_code: -1,
                    stderr: e.to_string(),
                }
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let exit_code = output.status.code().unwrap_or(-1);
        return Err(ServiceError::CommandFailed { exit_code, stderr });
    }

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    parse_journal_ndjson(&stdout)
}

// ── Parsing helpers ───────────────────────────────────────────────────────────

/// Parses NDJSON (newline-delimited JSON) produced by `journalctl --output=json`.
///
/// Each line in the output is a self-contained JSON object.  The function
/// skips blank lines and stops on the first parse error.
fn parse_journal_ndjson(ndjson: &str) -> Result<Vec<LogEntry>, ServiceError> {
    let mut entries = Vec::new();

    for line in ndjson.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let obj: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| ServiceError::ParseError(format!("invalid journal JSON: {e}")))?;

        entries.push(LogEntry {
            timestamp: parse_realtime_timestamp(&obj),
            message: parse_message(&obj),
            priority: parse_priority(&obj),
        });
    }

    Ok(entries)
}

/// Converts the `__REALTIME_TIMESTAMP` field (microseconds since Unix epoch)
/// to an ISO 8601 UTC string: `YYYY-MM-DDTHH:MM:SS.xxxxxxZ`.
fn parse_realtime_timestamp(obj: &serde_json::Value) -> String {
    let micros: u64 = obj
        .get("__REALTIME_TIMESTAMP")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let secs = micros / 1_000_000;
    let sub_micros = (micros % 1_000_000) as u32;

    let (year, month, day) = epoch_days_to_ymd(secs / 86400);
    let time_secs = secs % 86400;
    let hh = time_secs / 3600;
    let mm = (time_secs % 3600) / 60;
    let ss = time_secs % 60;

    format!("{year:04}-{month:02}-{day:02}T{hh:02}:{mm:02}:{ss:02}.{sub_micros:06}Z")
}

/// Converts days since the Unix epoch (1970-01-01) to `(year, month, day)`.
///
/// Uses Howard Hinnant's civil calendar algorithm which handles the full
/// Gregorian calendar correctly for any non-negative day count.
fn epoch_days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Extracts the `MESSAGE` field.
///
/// journald can encode binary messages as a JSON array of byte values; this
/// function handles both the plain-string and byte-array forms.
fn parse_message(obj: &serde_json::Value) -> String {
    match obj.get("MESSAGE") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(bytes)) => {
            let raw: Vec<u8> = bytes
                .iter()
                .filter_map(|b| b.as_u64().map(|n| n as u8))
                .collect();
            String::from_utf8_lossy(&raw).into_owned()
        }
        _ => String::new(),
    }
}

/// Maps the syslog `PRIORITY` field (`"0"`–`"7"`) to a [`LogPriority`] variant.
fn parse_priority(obj: &serde_json::Value) -> LogPriority {
    match obj
        .get("PRIORITY")
        .and_then(|v| v.as_str())
        .unwrap_or("6")
    {
        "0" => LogPriority::Emergency,
        "1" => LogPriority::Alert,
        "2" => LogPriority::Critical,
        "3" => LogPriority::Error,
        "4" => LogPriority::Warning,
        "5" => LogPriority::Notice,
        "7" => LogPriority::Debug,
        _ => LogPriority::Info, // covers "6" and any unexpected value
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epoch_days_to_ymd_epoch() {
        // days = 0 -> 1970-01-01
        assert_eq!(epoch_days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn test_epoch_days_to_ymd_known_date() {
        // 2024-01-01: 54 years after epoch
        // days = 365*54 + 14 leap days (72,76,80,...,2000,04,08,12,16,20,24 -> 14 extra)
        // Actually: 19723 days from 1970-01-01 to 2024-01-01
        assert_eq!(epoch_days_to_ymd(19_723), (2024, 1, 1));
    }

    #[test]
    fn test_parse_realtime_timestamp() {
        // 0 microseconds -> 1970-01-01T00:00:00.000000Z
        let obj = serde_json::json!({"__REALTIME_TIMESTAMP": "0"});
        assert_eq!(parse_realtime_timestamp(&obj), "1970-01-01T00:00:00.000000Z");
    }

    #[test]
    fn test_parse_message_string() {
        let obj = serde_json::json!({"MESSAGE": "hello world"});
        assert_eq!(parse_message(&obj), "hello world");
    }

    #[test]
    fn test_parse_message_bytes() {
        let obj = serde_json::json!({"MESSAGE": [104u8, 105u8]}); // "hi"
        assert_eq!(parse_message(&obj), "hi");
    }

    #[test]
    fn test_parse_priority() {
        let cases = [
            ("0", LogPriority::Emergency),
            ("1", LogPriority::Alert),
            ("2", LogPriority::Critical),
            ("3", LogPriority::Error),
            ("4", LogPriority::Warning),
            ("5", LogPriority::Notice),
            ("6", LogPriority::Info),
            ("7", LogPriority::Debug),
        ];
        for (raw, expected) in cases {
            let obj = serde_json::json!({"PRIORITY": raw});
            assert_eq!(parse_priority(&obj), expected);
        }
    }

    #[test]
    fn test_parse_journal_ndjson_basic() {
        let ndjson = r#"{"__REALTIME_TIMESTAMP":"1000000","MESSAGE":"boot","PRIORITY":"6"}
{"__REALTIME_TIMESTAMP":"2000000","MESSAGE":"started","PRIORITY":"5"}"#;

        let entries = parse_journal_ndjson(ndjson).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message, "boot");
        assert_eq!(entries[0].priority, LogPriority::Info);
        assert_eq!(entries[1].message, "started");
        assert_eq!(entries[1].priority, LogPriority::Notice);
    }

    #[test]
    fn test_parse_journal_ndjson_skips_blank_lines() {
        let ndjson = "\n{\"__REALTIME_TIMESTAMP\":\"0\",\"MESSAGE\":\"x\",\"PRIORITY\":\"6\"}\n\n";
        let entries = parse_journal_ndjson(ndjson).unwrap();
        assert_eq!(entries.len(), 1);
    }
}

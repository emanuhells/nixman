//! Line-by-line output streaming through an mpsc channel.
//!
//! [`stream_lines`] reads an async byte-stream line by line, detects build
//! phase transitions, and emits [`BuildEvent`]s through the given sender.
//! It returns all collected lines so callers can assemble the final output
//! string without a second pass.

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::mpsc;

use crate::builder::phases;
use crate::builder::types::BuildEvent;

/// Read `reader` line-by-line and forward each line as a [`BuildEvent`].
///
/// For every line the function:
/// 1. Detects whether the line signals a new build phase (via
///    [`phases::detect`]); if so, sends a [`BuildEvent::PhaseChanged`] first.
/// 2. Sends a [`BuildEvent::Output`] containing the trimmed line.
///
/// Send errors (e.g. a dropped receiver) are silently ignored so that a
/// disconnected frontend does not abort the build.
///
/// Returns all lines that were read so the caller can include them in the
/// final [`crate::builder::types::BuildResult`].
pub async fn stream_lines<R>(reader: R, tx: mpsc::Sender<BuildEvent>) -> Vec<String>
where
    R: AsyncRead + Unpin,
{
    let mut buf_reader = BufReader::new(reader);
    let mut raw_line = String::new();
    let mut collected: Vec<String> = Vec::new();

    loop {
        raw_line.clear();

        match buf_reader.read_line(&mut raw_line).await {
            // EOF — the child process closed the pipe.
            Ok(0) => break,
            Ok(_) => {
                // Strip the trailing newline (handles \n and \r\n).
                let line = raw_line
                    .trim_end_matches('\n')
                    .trim_end_matches('\r')
                    .to_string();

                // Detect build phase before forwarding the raw line.
                if let Some(phase) = phases::detect(&line) {
                    let _ = tx.send(BuildEvent::PhaseChanged(phase)).await;
                }

                // Forward the line as output.
                let _ = tx.send(BuildEvent::Output(line.clone())).await;
                collected.push(line);
            }
            // I/O error — stop reading but do not propagate; the process
            // exit status is the authoritative failure signal.
            Err(_) => break,
        }
    }

    collected
}

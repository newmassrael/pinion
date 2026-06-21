//! R833 §3 §5.53 §5.15 — CUPS bridge for the [`pinion_core::print`]
//! [`PrintBackend`] trait.
//!
//! [`CupsPrintBackend`] is the FileStorage-equivalent platform impl:
//! it enumerates destinations through `lpstat -e` / `lpstat -d` and
//! submits jobs through `lp` (copies / title / IPP orientation), parsing
//! the CUPS request id back into a [`PrintReceipt`]. No external crate —
//! CUPS ships the `lp` / `lpstat` CLI on every desktop Linux and macOS,
//! the canonical headless-driveable print interface (the own-renderer
//! contract, §5.53, keeps the *dialog* in pinion; only the spool bridge
//! is platform).
//!
//! When no CUPS destination is configured (this CI box), the binding
//! falls back to [`pinion_core::print::InMemoryPrintBackend`] so the
//! own-rendered dialog stays demonstrable — the same in-memory ⇄ platform
//! split as R665 Storage.

use std::io::Write;
use std::process::{Command, Stdio};

use pinion_core::print::{PrintBackend, PrintError, PrintJob, PrintReceipt, PrinterInfo};

/// IPP `orientation-requested` value for portrait.
const IPP_PORTRAIT: &str = "3";
/// IPP `orientation-requested` value for landscape.
const IPP_LANDSCAPE: &str = "4";

/// R833 §3 §5.53 — the CUPS-backed [`PrintBackend`].
#[derive(Debug, Default, Clone, Copy)]
pub struct CupsPrintBackend;

impl CupsPrintBackend {
    /// Construct the CUPS backend (stateless — every call shells out).
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// `true` when the CUPS scheduler is reachable (`lpstat -r` reports the
/// scheduler running). Bindings consult this to choose `CupsPrintBackend`
/// over the in-memory fallback.
#[must_use]
pub fn cups_available() -> bool {
    Command::new("lpstat")
        .arg("-r")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The CUPS system default destination, parsed from `lpstat -d`
/// ("system default destination: NAME"), or `None`.
fn cups_default_printer() -> Option<String> {
    let out = Command::new("lpstat").arg("-d").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (_, name) = text.trim().split_once(": ")?;
    let name = name.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

/// Parse the CUPS `lp` confirmation ("request id is NAME-42 (1 file(s))")
/// into the request id token.
fn parse_request_id(stdout: &str) -> Option<String> {
    let after = stdout.split("request id is ").nth(1)?;
    let id = after.split_whitespace().next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_owned())
    }
}

impl PrintBackend for CupsPrintBackend {
    fn enumerate_printers(&self) -> Vec<PrinterInfo> {
        let Ok(out) = Command::new("lpstat").arg("-e").output() else {
            return Vec::new();
        };
        if !out.status.success() {
            return Vec::new();
        }
        let default = cups_default_printer();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|name| PrinterInfo::new(name, name, default.as_deref() == Some(name)))
            .collect()
    }

    fn submit(&self, printer_id: &str, job: &PrintJob) -> Result<PrintReceipt, PrintError> {
        let orientation = match job.orientation {
            pinion_core::print::Orientation::Portrait => IPP_PORTRAIT,
            pinion_core::print::Orientation::Landscape => IPP_LANDSCAPE,
        };
        let mut child = Command::new("lp")
            .arg("-d")
            .arg(printer_id)
            .arg("-n")
            .arg(job.copies.to_string())
            .arg("-t")
            .arg(&job.title)
            .arg("-o")
            .arg(format!("orientation-requested={orientation}"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| PrintError::Backend(format!("lp spawn failed: {e}")))?;
        // Pipe the content to lp's stdin, then close it so lp proceeds.
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(job.content.as_bytes())
                .map_err(|e| PrintError::Backend(format!("lp stdin write failed: {e}")))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| PrintError::Backend(format!("lp wait failed: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(PrintError::Backend(format!(
                "lp exited with {}: {}",
                out.status,
                stderr.trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let job_id = parse_request_id(&stdout).unwrap_or_else(|| format!("{printer_id}-unknown"));
        Ok(PrintReceipt {
            printer_id: printer_id.to_owned(),
            copies: job.copies,
            job_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request_id_extracts_token() {
        assert_eq!(
            parse_request_id("request id is office-laser-42 (1 file(s))").as_deref(),
            Some("office-laser-42"),
        );
        assert_eq!(parse_request_id("no id here"), None);
    }

    #[test]
    fn enumerate_printers_runs_without_panicking() {
        // On a box with no CUPS destinations this returns an empty Vec;
        // on a configured box it returns the real roster. Either way the
        // call must not panic and the result must be well-formed (no
        // empty ids, no duplicate default).
        let backend = CupsPrintBackend::new();
        let printers = backend.enumerate_printers();
        assert!(printers.iter().all(|p| !p.id.is_empty()));
        assert!(printers.iter().filter(|p| p.is_default).count() <= 1);
    }

    #[test]
    fn cups_available_is_a_bool_probe() {
        // Just exercises the probe path; the value depends on whether
        // cupsd is running in this environment.
        let _ = cups_available();
    }
}

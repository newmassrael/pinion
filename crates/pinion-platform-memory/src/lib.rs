//! R1550 §5.7 §2 #2 — what the OS says this process is holding.
//!
//! # Why this sits beside the arena census
//!
//! `pinion_core::memory_census::MemoryCensus` prices the arenas pinion owns:
//! the paint fragments, the shaped text, the decoded images. Those will never
//! sum to what the process is resident for — the widget tree, taffy's layout
//! nodes, the font collection, the GPU driver's own buffers, and the binary
//! itself are all outside them — and a per-arena report with no total invites
//! exactly the wrong reading, that the arenas *are* the process.
//!
//! Reporting both makes the unattributed remainder visible as a subtraction
//! rather than leaving it implied. The engine does the same thing: `stat memory` shows
//! the platform's numbers beside the allocator's. The toolkit 6.11 publishes
//! neither — there is no process-memory API in the toolkit at all, and `cacheLimit()` is
//! a budget you set rather than a measurement of anything.
//!
//! # Platform coverage
//!
//! Linux only, today, and the absence elsewhere is stated rather than faked:
//! [`process_rss_bytes`] answers `None`, which travels to the wire as `null`
//! and never as a zero. macOS wants `task_info(TASK_BASIC_INFO)` and Windows
//! `GetProcessMemoryInfo`; both need a platform crate dependency this one
//! deliberately does not carry yet, and neither can be verified from here
//! (the same CI-runner gate the rest of the OS-native axis sits behind).
//!
//! # Why `/proc/self/status` and not `/proc/self/statm`
//!
//! `statm` reports resident **pages**, so converting it needs the page size,
//! which needs `sysconf` — a libc dependency and an `unsafe` call, in a
//! workspace that forbids `unsafe_code`. `status` reports `VmRSS` in kB
//! directly. One parse, no FFI, no unsafe.

/// Resident set size the OS attributes to this process, in bytes.
///
/// `None` when the platform has no reader wired (everything but Linux), or
/// when `/proc` is unreadable — a container with a masked `/proc`, for
/// instance. Both are the same answer for the same reason: this is a
/// measurement, and an unavailable measurement is not a zero.
#[must_use]
pub fn process_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        parse_vm_rss(&std::fs::read_to_string("/proc/self/status").ok()?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// The `VmRSS` line of a `/proc/<pid>/status` body, in bytes.
///
/// Split out from the read so the parse is testable without a process whose
/// size we control: the format is `VmRSS:\t   12345 kB`, and getting the unit
/// wrong is a 1024x error that a live read cannot detect.
#[must_use]
pub fn parse_vm_rss(status: &str) -> Option<u64> {
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let mut fields = line.split_whitespace().skip(1);
    let value: u64 = fields.next()?.parse().ok()?;
    match fields.next() {
        // The kernel has written kB here since 2.6 and there is no in-tree
        // path that writes anything else, but a unit we do not recognise is
        // answered with `None` rather than by assuming.
        Some("kB") => Some(value * 1024),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r1550_parses_the_kernel_format() {
        let status = "Name:\tpinion\nVmPeak:\t  200000 kB\nVmRSS:\t   12345 kB\nThreads:\t8\n";
        assert_eq!(parse_vm_rss(status), Some(12_345 * 1024));
    }

    /// The unit is load-bearing and a wrong one is a silent 1024x. A line
    /// without it is refused rather than read as bytes.
    #[test]
    fn r1550_an_unknown_unit_is_not_guessed() {
        assert_eq!(parse_vm_rss("VmRSS:\t   12345\n"), None);
        assert_eq!(parse_vm_rss("VmRSS:\t   12345 MB\n"), None);
        assert_eq!(parse_vm_rss("VmHWM:\t   12345 kB\n"), None);
        assert_eq!(parse_vm_rss(""), None);
    }

    /// The one thing a parse test cannot cover: that the file exists, is
    /// readable, and has the line. Asserted as a floor rather than a value —
    /// any process that has loaded a test harness is resident for more than
    /// a megabyte, and no host makes that false.
    #[cfg(target_os = "linux")]
    #[test]
    fn r1550_this_process_reports_a_plausible_rss() {
        let rss = process_rss_bytes().expect("Linux reports VmRSS");
        assert!(
            rss > 1024 * 1024,
            "a running test binary is resident for more than 1 MiB, got {rss}",
        );
    }
}

//! R833 §3 §5.53 §5.15 — print substrate: the own-rendered print-dialog
//! backend contract.
//!
//! pinion paints its **own** print dialog (no native GTK / the toolkit / Win32
//! print dialog — those are out of the own-renderer scope, §5.53) and,
//! when the user confirms, hands a [`PrintJob`] to a [`PrintBackend`].
//! Mirrors the R665 [`Storage`](crate::storage) trait pattern exactly:
//!
//! - [`PrintBackend`] — the bytes-thin contract (enumerate + submit).
//! - [`InMemoryPrintBackend`] — pure-Rust, records submitted jobs; the
//!   canonical test / headless fixture (a fixed printer roster).
//! - `pinion_platform_print::CupsPrintBackend` — the real Linux bridge
//!   (`lpstat` enumerate + `lp` submit), the FileStorage-equivalent.
//!
//! The job [`content`](PrintJob::content) is plain text this round; a
//! scene → PDF / PostScript page-render pipeline is a documented future
//! axis (the backend already accepts arbitrary content, so that lands
//! additively without reshaping the contract).

use std::cell::{Cell, RefCell};

/// Page orientation for a [`PrintJob`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Orientation {
    /// Taller than wide (the default).
    #[default]
    Portrait,
    /// Wider than tall.
    Landscape,
}

impl Orientation {
    /// Stable lowercase token (the CUPS `-o orientation-requested` /
    /// introspect wire form).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
        }
    }

    /// Toggle between the two orientations.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Portrait => Self::Landscape,
            Self::Landscape => Self::Portrait,
        }
    }

    /// Parse the canonical lowercase wire token — the read inverse of
    /// [`Self::as_str`] ([[wire-form-read-write-symmetry]]). `None` for
    /// any other string. The one home of the `"portrait"` / `"landscape"`
    /// vocabulary, so consumers that read orientation off the wire
    /// (`scene/export_pdf`, R908) cannot drift from the spool token
    /// [`Self::as_str`] writes.
    #[must_use]
    pub fn from_wire(token: &str) -> Option<Self> {
        match token {
            "portrait" => Some(Self::Portrait),
            "landscape" => Some(Self::Landscape),
            _ => None,
        }
    }
}

/// A printer a [`PrintBackend`] can target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrinterInfo {
    /// Backend-stable identifier (the CUPS destination name; the
    /// `printer_id` passed to [`PrintBackend::submit`]).
    pub id: String,
    /// Human-readable name shown in the dialog.
    pub name: String,
    /// `true` for the system default destination (pre-selected).
    pub is_default: bool,
}

impl PrinterInfo {
    /// Construct a printer descriptor.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, is_default: bool) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            is_default,
        }
    }
}

/// A configured print job: what to print and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintJob {
    /// Document title shown in the print queue.
    pub title: String,
    /// The content to spool. Plain text this round (PDF / PostScript
    /// page render is a future axis).
    pub content: String,
    /// Number of copies (clamped `>= 1` by [`Self::with_copies`]).
    pub copies: u32,
    /// Page orientation.
    pub orientation: Orientation,
}

impl PrintJob {
    /// A one-copy portrait job for `content` titled `title`.
    #[must_use]
    pub fn new(title: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            content: content.into(),
            copies: 1,
            orientation: Orientation::Portrait,
        }
    }

    /// Builder: set the copy count (clamped to `>= 1`).
    #[must_use]
    pub fn with_copies(mut self, copies: u32) -> Self {
        self.copies = copies.max(1);
        self
    }

    /// Builder: set the orientation.
    #[must_use]
    pub const fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }
}

/// The receipt a successful [`PrintBackend::submit`] returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrintReceipt {
    /// The destination the job went to.
    pub printer_id: String,
    /// Copies submitted (echoed from the job).
    pub copies: u32,
    /// Backend-assigned job identifier (e.g. CUPS `request id`).
    pub job_id: String,
}

/// Why a [`PrintBackend::submit`] failed. Total surface — the dialog
/// renders the variant; nothing panics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrintError {
    /// The backend has no destinations configured.
    NoPrintersAvailable,
    /// `printer_id` is not in [`PrintBackend::enumerate_printers`].
    NoSuchPrinter,
    /// The backend command failed; carries its diagnostic.
    Backend(String),
}

/// R833 §3 §5.53 — the print backend contract: enumerate destinations
/// and submit jobs. Mirrors [`Storage`](crate::storage::Storage) (a
/// small total trait with an in-memory + a platform impl).
///
/// `Debug` is a super-trait so a runtime-selected `Box<dyn PrintBackend>`
/// (the binding's "CUPS if available, else in-memory" choice — mirroring
/// todomvc's `AppStorage`) participates in `#[derive(Debug)]`.
pub trait PrintBackend: core::fmt::Debug {
    /// The destinations this backend can print to. May be empty (no
    /// printer configured) — the dialog shows the empty state.
    fn enumerate_printers(&self) -> Vec<PrinterInfo>;

    /// Submit `job` to the destination `printer_id`. Returns the
    /// [`PrintReceipt`] on success.
    ///
    /// # Errors
    ///
    /// - [`PrintError::NoPrintersAvailable`] when the backend has no
    ///   destinations configured.
    /// - [`PrintError::NoSuchPrinter`] when `printer_id` is not in
    ///   [`enumerate_printers`](Self::enumerate_printers).
    /// - [`PrintError::Backend`] when the platform print command fails.
    fn submit(&self, printer_id: &str, job: &PrintJob) -> Result<PrintReceipt, PrintError>;
}

/// R833 §3 §5.53 — pure-Rust in-memory [`PrintBackend`]: a fixed printer
/// roster plus a record of every submitted job. The canonical test /
/// headless fixture (the `CupsPrintBackend` equivalent of
/// [`InMemoryStorage`](crate::storage::InMemoryStorage)) — and the
/// fallback a binding uses when no real CUPS destination exists (this CI
/// box has none), so the own-rendered dialog stays demonstrable.
#[derive(Debug)]
pub struct InMemoryPrintBackend {
    printers: Vec<PrinterInfo>,
    submitted: RefCell<Vec<(String, PrintJob)>>,
    next_job: Cell<u32>,
}

impl InMemoryPrintBackend {
    /// Construct with an explicit printer roster.
    #[must_use]
    pub fn new(printers: Vec<PrinterInfo>) -> Self {
        Self {
            printers,
            submitted: RefCell::new(Vec::new()),
            next_job: Cell::new(1),
        }
    }

    /// A three-printer sample roster for demos / headless runs (the
    /// first is the default).
    #[must_use]
    pub fn with_sample_printers() -> Self {
        Self::new(vec![
            PrinterInfo::new("office-laser", "Office Laser", true),
            PrinterInfo::new("pdf-virtual", "PDF (virtual)", false),
            PrinterInfo::new("front-desk", "Front Desk Color", false),
        ])
    }

    /// Number of jobs submitted so far. Test / introspection helper.
    #[must_use]
    pub fn submitted_count(&self) -> usize {
        self.submitted.borrow().len()
    }

    /// The most recently submitted `(printer_id, job)`, if any.
    #[must_use]
    pub fn last_submitted(&self) -> Option<(String, PrintJob)> {
        self.submitted.borrow().last().cloned()
    }
}

impl PrintBackend for InMemoryPrintBackend {
    fn enumerate_printers(&self) -> Vec<PrinterInfo> {
        self.printers.clone()
    }

    fn submit(&self, printer_id: &str, job: &PrintJob) -> Result<PrintReceipt, PrintError> {
        if self.printers.is_empty() {
            return Err(PrintError::NoPrintersAvailable);
        }
        if !self.printers.iter().any(|p| p.id == printer_id) {
            return Err(PrintError::NoSuchPrinter);
        }
        let n = self.next_job.get();
        self.next_job.set(n + 1);
        self.submitted
            .borrow_mut()
            .push((printer_id.to_owned(), job.clone()));
        Ok(PrintReceipt {
            printer_id: printer_id.to_owned(),
            copies: job.copies,
            job_id: format!("mem-{n}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> InMemoryPrintBackend {
        InMemoryPrintBackend::with_sample_printers()
    }

    #[test]
    fn orientation_tokens_and_toggle() {
        assert_eq!(Orientation::Portrait.as_str(), "portrait");
        assert_eq!(Orientation::Landscape.as_str(), "landscape");
        assert_eq!(Orientation::Portrait.toggled(), Orientation::Landscape);
        assert_eq!(Orientation::Landscape.toggled(), Orientation::Portrait);
        assert_eq!(Orientation::default(), Orientation::Portrait);
    }

    #[test]
    fn orientation_from_wire_inverts_as_str() {
        for o in [Orientation::Portrait, Orientation::Landscape] {
            assert_eq!(
                Orientation::from_wire(o.as_str()),
                Some(o),
                "round-trip {o:?}"
            );
        }
        assert_eq!(Orientation::from_wire("sideways"), None);
        assert_eq!(
            Orientation::from_wire("Portrait"),
            None,
            "strict lowercase token"
        );
    }

    #[test]
    fn job_builder_clamps_copies() {
        let job = PrintJob::new("Doc", "hello").with_copies(0);
        assert_eq!(job.copies, 1, "copies clamps to >= 1");
        let job = PrintJob::new("Doc", "hello").with_copies(5);
        assert_eq!(job.copies, 5);
        assert_eq!(job.orientation, Orientation::Portrait);
    }

    #[test]
    fn sample_roster_has_one_default() {
        let b = sample();
        let printers = b.enumerate_printers();
        assert_eq!(printers.len(), 3);
        assert_eq!(printers.iter().filter(|p| p.is_default).count(), 1);
        assert!(
            printers[0].is_default,
            "first sample printer is the default"
        );
    }

    #[test]
    fn submit_records_job_and_returns_receipt() {
        let b = sample();
        assert_eq!(b.submitted_count(), 0);
        let job = PrintJob::new("Report", "page one").with_copies(3);
        let receipt = b.submit("pdf-virtual", &job).expect("submit succeeds");
        assert_eq!(receipt.printer_id, "pdf-virtual");
        assert_eq!(receipt.copies, 3);
        assert_eq!(receipt.job_id, "mem-1");
        assert_eq!(b.submitted_count(), 1);
        let (pid, recorded) = b.last_submitted().expect("a recorded job");
        assert_eq!(pid, "pdf-virtual");
        assert_eq!(recorded.copies, 3);
        assert_eq!(recorded.content, "page one");
        // A second submit increments the job id.
        let r2 = b.submit("office-laser", &job).expect("second submit");
        assert_eq!(r2.job_id, "mem-2");
        assert_eq!(b.submitted_count(), 2);
    }

    #[test]
    fn submit_rejects_unknown_printer() {
        let b = sample();
        assert_eq!(
            b.submit("ghost", &PrintJob::new("D", "c")),
            Err(PrintError::NoSuchPrinter),
        );
        assert_eq!(b.submitted_count(), 0, "a rejected submit records nothing");
    }

    #[test]
    fn submit_rejects_when_no_printers() {
        let b = InMemoryPrintBackend::new(Vec::new());
        assert_eq!(
            b.submit("anything", &PrintJob::new("D", "c")),
            Err(PrintError::NoPrintersAvailable),
        );
    }

    #[test]
    fn backend_is_object_safe() {
        let b: Box<dyn PrintBackend> = Box::new(sample());
        assert_eq!(b.enumerate_printers().len(), 3);
        assert!(b.submit("office-laser", &PrintJob::new("D", "c")).is_ok());
    }
}

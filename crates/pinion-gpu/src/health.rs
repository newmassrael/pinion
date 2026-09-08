//! R1709 §5.16 — whether a window is putting frames on the screen, why not
//! when it is not, and which rung of the recovery ladder that earns.
//!
//! # Why this exists
//!
//! Before this round the whole of pinion's answer to a swapchain that would
//! not hand over an image was one line: reconfigure the surface, skip the
//! frame, and try again next frame. Its comment (R1049) asserted that this
//! "re-establishes the swapchain so the NEXT frame acquires a fresh
//! texture" — and nothing ever checked whether the next frame did.
//!
//! Measured, on a window that was never mapped: it does not. Every acquire
//! came back outdated, the reconfigure ran again, and the window never
//! presented another frame for the rest of its life — eight consecutive
//! failures across two resizes, with `scene/screenshot` dead throughout. A
//! recovery that never verifies it recovered is not a recovery; it is a
//! loop that reads like one.
//!
//! So a failure is no longer a single response repeated. It is a **ladder**,
//! and the rung is chosen by [`SurfaceHealth`] from one fact: how many
//! attempts in a row have failed since the last frame reached the screen.
//! That makes "did the previous rung work?" structural rather than a thing
//! someone has to remember to ask — if a rung worked, the next frame
//! presents, the count resets, and the ladder is never climbed.
//!
//! # Two counters, because they answer different questions
//!
//! A frame can miss the screen because the surface is **broken**
//! (invalidated: it will not present again until something is rebuilt) or
//! because the window is merely **waiting** (occluded, or the image did not
//! arrive in time). Both are frames a viewer did not see, so both belong to
//! "how long since this window presented". Only the first is evidence that
//! recovery is needed, so only the first moves the ladder.
//!
//! Collapsing them would break the ladder in a way that is easy to miss: a
//! window that sat occluded for five frames and then genuinely broke would
//! skip straight past the cheap rung to the exhausted one, having never
//! tried the response that fixes the ordinary case.

/// Why a frame did not reach the screen.
///
/// One arm per non-presentable acquisition status `wgpu` distinguishes.
/// Kept as pinion's own enum rather than re-exporting `wgpu`'s: the
/// question this answers ("what does a viewer not see, and is it
/// recoverable") is the framework's, and it is published on the wire, where
/// a backend's type has no business.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Missed {
    /// The swapchain no longer matches the surface it was made from —
    /// canonically because the window was resized.
    Outdated,
    /// The swapchain was lost outright.
    Lost,
    /// The driver rejected the acquisition.
    Validation,
    /// No image became available in time. A wait, not a breakage.
    Timeout,
    /// Nothing is looking at this window. A wait, not a breakage.
    Occluded,
    /// R2088 — the surface is **not configured for presentation**, so no
    /// image was asked for at all.
    ///
    /// The one arm pinion raises itself rather than reading off a `wgpu`
    /// status, and the reason it has to is that asking is what kills the
    /// process: `wgpu`'s `get_current_texture()` on a surface whose
    /// `configure` never succeeded reports through
    /// `handle_error_fatal`, which consults no uncaptured-error handler
    /// and panics. Every other arm here is a status `wgpu` was willing to
    /// hand back.
    ///
    /// It is an invalidation ([`Self::is_invalidation`]), because a
    /// surface in this state will never present again on its own — the
    /// recovery ladder is exactly what gets it out.
    Unconfigured,
    /// R2088.1 — the **device** behind this surface has been lost, so
    /// nothing on it can present and nothing here can ask.
    ///
    /// A separate arm from [`Self::Lost`], which is the *swapchain* going
    /// away on a device that is still working. Collapsing them would erase
    /// the only distinction that matters to a reader: a lost swapchain is
    /// remade by the ladder's heavy rung, and a lost device is not remade
    /// by anything this type can do.
    ///
    /// ★ It is also the one arm that arrives on a channel an error scope
    /// **cannot** see. `wgpu`'s `handle_error_inner` matches
    /// `ErrorType::DeviceLost => return` — no sink, so no scope and no
    /// uncaptured handler — and says why beside it: *will be surfaced via
    /// callback*. See [`crate::DeviceLiveness`].
    DeviceLost,
}

impl Missed {
    /// Every arm, so a census or a doc table derives its rows instead of
    /// hand-listing them.
    pub const ALL: [Self; 7] = [
        Self::Outdated,
        Self::Lost,
        Self::Validation,
        Self::Timeout,
        Self::Occluded,
        Self::Unconfigured,
        Self::DeviceLost,
    ];

    /// Where this reason's count lives in a [`MissTally`].
    ///
    /// Written as a wildcard-free match rather than a search through
    /// [`Self::ALL`], so a new arm cannot be added without the compiler
    /// asking where it is counted. The two declarations are held together
    /// by a test that walks `ALL` and asserts each arm indexes back to
    /// itself — neither can drift without something failing.
    #[must_use]
    const fn slot(self) -> usize {
        match self {
            Self::Outdated => 0,
            Self::Lost => 1,
            Self::Validation => 2,
            Self::Timeout => 3,
            Self::Occluded => 4,
            Self::Unconfigured => 5,
            Self::DeviceLost => 6,
        }
    }

    /// Whether this is the surface *breaking* — the case a recovery can act
    /// on — rather than the window waiting.
    ///
    /// This is the discrimination the recovery ladder is built on: waiting
    /// is not evidence that anything needs rebuilding, and treating it as
    /// such would spend the ladder's rungs on a window that is fine.
    #[must_use]
    pub fn is_invalidation(self) -> bool {
        match self {
            Self::Outdated
            | Self::Lost
            | Self::Validation
            | Self::Unconfigured
            | Self::DeviceLost => true,
            Self::Timeout | Self::Occluded => false,
        }
    }

    /// The image an acquisition handed over, or why it did not.
    ///
    /// Lives here so the mapping is written once. It had been written
    /// twice — the emitted renderer's `render` and the shell's screenshot
    /// capture each spelled all six arms out — which is two chances for a
    /// status to be classified differently on the two paths a frame can
    /// take to the same screen.
    ///
    /// ★ R2088 — it now takes the status **by value and hands the texture
    /// back**, so the classification and the "was there an image?" question
    /// are one `match` instead of two. Until this round it answered
    /// `Option<Missed>` from a borrow, and each of the two callers then
    /// re-destructured the same enum to reach the texture — which is why
    /// both carried an arm labelled *unclassified* and documented as
    /// unreachable. An arm that exists because two matches could disagree
    /// is that disagreement, written down.
    ///
    /// # Errors
    ///
    /// The [`Missed`](Self) an acquisition that handed over no image names.
    /// Never [`Self::Unconfigured`], which is not a status `wgpu` can
    /// answer — see that variant.
    pub fn split(status: wgpu::CurrentSurfaceTexture) -> Result<wgpu::SurfaceTexture, Self> {
        match status {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Ok(texture),
            wgpu::CurrentSurfaceTexture::Timeout => Err(Self::Timeout),
            wgpu::CurrentSurfaceTexture::Occluded => Err(Self::Occluded),
            wgpu::CurrentSurfaceTexture::Outdated => Err(Self::Outdated),
            wgpu::CurrentSurfaceTexture::Lost => Err(Self::Lost),
            wgpu::CurrentSurfaceTexture::Validation => Err(Self::Validation),
        }
    }

    /// The wire spelling, and what a log line says.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Outdated => "outdated",
            Self::Lost => "lost",
            Self::Validation => "validation",
            Self::Timeout => "timeout",
            Self::Occluded => "occluded",
            Self::Unconfigured => "unconfigured",
            Self::DeviceLost => "device_lost",
        }
    }
}

impl core::fmt::Display for Missed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// ★ R2088.1 §5.16 — whether the device behind a surface is still usable.
///
/// # Why this type exists at all
///
/// R2088 made a refused `Surface::configure` a *measured* fact by running it
/// inside `wgpu` error scopes. Measured again against `wgpu` 29's source, that
/// is not a complete failure channel:
///
/// ```text
/// // wgpu-29.0.3/src/backend/wgpu_core.rs, handle_error_inner
/// ErrorType::DeviceLost => return, // will be surfaced via callback
/// ```
///
/// That arm never touches the error sink, so **no scope and no
/// uncaptured-error handler can see it** — and `configure_surface` reaches it
/// whenever the failure is a `DeviceError::Lost` (its `check_is_valid()` and
/// its `maintain()`, and `hal::DeviceError::Unexpected` maps to `Lost` too).
/// So a scope that caught nothing does **not** mean the configure worked, and
/// R2088's first draft read that silence as success — which is the very
/// inference this debt is about, moved one layer up.
///
/// This is the callback's landing place, which is the channel `wgpu`'s own
/// comment points at. There is no second opinion available: `Device::poll`
/// cannot serve as a liveness probe, because a lost device makes it call
/// `handle_error_fatal` (measured, same file) — a probe would be a *third*
/// way to abort rather than a way to avoid one.
///
/// # Shape
///
/// Shared, not copied: the callback holds one handle and every surface on that
/// device holds another, and they must be **one fact**. That is what
/// [`Self::is_lost`] is asserted on without a GPU.
#[derive(Debug, Clone, Default)]
pub struct DeviceLiveness(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl DeviceLiveness {
    /// Record that the device has been lost. Called from `wgpu`'s
    /// device-lost callback, which may fire on any thread.
    pub(crate) fn lose(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }

    /// Whether the device has been lost.
    ///
    /// Read at the **acquire**, not cached from configure time: the callback
    /// fires from `wgpu`'s maintain, and `configure_surface` does not fire its
    /// user callbacks on the path where it fails — so the honest moment to ask
    /// is the moment before asking `wgpu` for an image.
    #[must_use]
    pub fn is_lost(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// Which rung of the recovery ladder a broken surface earned.
///
/// Ordered by cost, and climbed only on evidence: a rung is reached when
/// every cheaper one has already been taken and the window still has not
/// presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rung {
    /// Re-establish the swapchain from the surface that already exists.
    /// `wgpu`'s documented response to a stale swapchain, correct for the
    /// ordinary case (a resized, mapped window), and the entire ladder
    /// before R1709.
    Reconfigured,
    /// Throw the surface itself away and make another one for the same
    /// window. Reached only when [`Self::Reconfigured`] was taken and the
    /// very next attempt failed anyway — which is exactly the state a
    /// reconfigure cannot get out of.
    Rebuilt,
    /// The ladder has nothing new left, so the cheap rung is being repeated.
    ///
    /// Deliberately *not* "give up": a surface that is only reachable
    /// through a reconfigure would be dead forever if this stopped trying,
    /// which is worse than the defect this file exists for. What changes at
    /// this rung is what is *published* — that everything known has been
    /// tried and the window still is not presenting.
    Repeated,
}

impl Rung {
    /// Every arm, in ladder order.
    pub const ALL: [Self; 3] = [Self::Reconfigured, Self::Rebuilt, Self::Repeated];

    /// Where this rung's count lives in a [`RungTally`]. See
    /// [`Missed::slot`] for why this is a match rather than a search.
    #[must_use]
    const fn slot(self) -> usize {
        match self {
            Self::Reconfigured => 0,
            Self::Rebuilt => 1,
            Self::Repeated => 2,
        }
    }

    /// The wire spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reconfigured => "reconfigured",
            Self::Rebuilt => "rebuilt",
            Self::Repeated => "repeated",
        }
    }
}

/// R2090 — how many frames this window has missed for each reason, over its
/// whole life.
///
/// # Why a cumulative tally exists at all
///
/// Measured against this file before R2090: [`SurfaceHealth::presented`]
/// resets `missed_in_a_row`, `broken_in_a_row`, `last_missed` and
/// `last_rung`, so a window that broke and then recovered leaves **no trace
/// a later reader can find**. The single survivor was the rebuild count,
/// and it is incremented on one rung only — so the cheap rung, which is the
/// common one, had never been counted anywhere.
///
/// That is a defect of the instrument rather than of the renderer, and it is
/// the one that matters here: the open defect this vocabulary exists for is
/// intermittent, so judging a repair needs a **denominator** — how often a
/// window came near the fatal path at all — and a counter that forgets on
/// recovery cannot supply one. A run can now be counted after it ends
/// instead of only watched while it happens.
///
/// Rows are keyed by [`Missed::ALL`] order, so a reader derives the table
/// rather than hand-listing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MissTally([u32; Missed::ALL.len()]);

impl MissTally {
    /// Count one missed frame.
    pub fn note(&mut self, missed: Missed) {
        let slot = &mut self.0[missed.slot()];
        *slot = slot.saturating_add(1);
    }

    /// How many frames this window has missed for `missed`.
    #[must_use]
    pub fn of(self, missed: Missed) -> u32 {
        self.0[missed.slot()]
    }

    /// Every frame this window has missed, for any reason.
    #[must_use]
    pub fn total(self) -> u32 {
        self.0.iter().fold(0u32, |sum, n| sum.saturating_add(*n))
    }

    /// Of those, the ones that were the surface *breaking* rather than the
    /// window waiting — the cumulative twin of
    /// [`SurfaceHealth::broken_in_a_row`].
    ///
    /// Derived from [`Missed::is_invalidation`], the same predicate the
    /// ladder is built on, so the two can never disagree about what counts
    /// as a breakage.
    #[must_use]
    pub fn breakages(self) -> u32 {
        self.rows()
            .into_iter()
            .filter(|(missed, _)| missed.is_invalidation())
            .fold(0u32, |sum, (_, n)| sum.saturating_add(n))
    }

    /// Every reason paired with its count, in [`Missed::ALL`] order.
    ///
    /// The rows a publisher writes out — derived here so no consumer has to
    /// spell the vocabulary a second time.
    #[must_use]
    pub fn rows(self) -> [(Missed, u32); Missed::ALL.len()] {
        Missed::ALL.map(|missed| (missed, self.of(missed)))
    }
}

/// R2090 — how many times each rung of the recovery ladder has been taken
/// over this window's whole life. See [`MissTally`] for why the cumulative
/// form is the one a reader needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RungTally([u32; Rung::ALL.len()]);

impl RungTally {
    /// Count one rung taken.
    pub fn note(&mut self, rung: Rung) {
        let slot = &mut self.0[rung.slot()];
        *slot = slot.saturating_add(1);
    }

    /// How many times `rung` has been taken.
    #[must_use]
    pub fn of(self, rung: Rung) -> u32 {
        self.0[rung.slot()]
    }

    /// Every rung paired with its count, in ladder order.
    #[must_use]
    pub fn rows(self) -> [(Rung, u32); Rung::ALL.len()] {
        Rung::ALL.map(|rung| (rung, self.of(rung)))
    }
}

impl core::fmt::Display for Rung {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a window can say about putting frames on the screen.
///
/// Held per surface and published on the wire, so an agent driving the
/// window over RPC can tell a one-frame blip from a window that has not
/// presented in a hundred frames — a distinction a per-frame `present_ok`
/// boolean cannot make, and which was the whole reason a permanently dead
/// surface went unnoticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SurfaceHealth {
    missed_in_a_row: u32,
    broken_in_a_row: u32,
    last_missed: Option<Missed>,
    last_rung: Option<Rung>,
    // R2090 — the two cumulative halves. Every field above resets when a
    // frame reaches the screen; these do not, so a reader arriving after a
    // recovery can still say what this window has been through. The rebuild
    // count that used to live here is now `rungs.of(Rung::Rebuilt)`: it was
    // one rung's tally written out a second time, and two counts of one fact
    // can disagree.
    misses: MissTally,
    rungs: RungTally,
}

impl SurfaceHealth {
    /// Frames that did not reach the screen since the last one that did,
    /// counting waits as well as breakages. `0` ⟺ the last attempt
    /// presented.
    #[must_use]
    pub fn missed_in_a_row(&self) -> u32 {
        self.missed_in_a_row
    }

    /// Of those, how many were the surface *breaking* rather than waiting.
    /// This is what selects the rung.
    #[must_use]
    pub fn broken_in_a_row(&self) -> u32 {
        self.broken_in_a_row
    }

    /// Why the most recent missed frame missed, or `None` when the last
    /// attempt presented.
    #[must_use]
    pub fn last_missed(&self) -> Option<Missed> {
        self.last_missed
    }

    /// The rung taken for the most recent breakage, or `None` when nothing
    /// is currently broken.
    #[must_use]
    pub fn last_rung(&self) -> Option<Rung> {
        self.last_rung
    }

    /// How many times this window's surface has had to be remade, over the
    /// window's whole life.
    ///
    /// Cumulative on purpose — it does NOT reset when the window recovers.
    /// A window that is healthy *now* having needed four rebuilds to get
    /// there is a different fact from one that has needed none.
    ///
    /// R2090 — derived from [`Self::rungs`] rather than counted separately.
    /// Until this round it was its own field incremented beside the rung it
    /// names, which is one fact with two writers; it is now the heavy rung's
    /// row, and [`Self::misses`] answers the questions it used to be the
    /// only survivor of.
    #[must_use]
    pub fn rebuilds(&self) -> u32 {
        self.rungs.of(Rung::Rebuilt)
    }

    /// Every frame this window has missed, by reason, over its whole life.
    ///
    /// The cumulative half of this type: see [`MissTally`] for why an
    /// instrument that forgets on recovery cannot supply the denominator
    /// this vocabulary's open defect needs.
    #[must_use]
    pub fn misses(&self) -> MissTally {
        self.misses
    }

    /// Every rung of the recovery ladder this window has taken, over its
    /// whole life.
    #[must_use]
    pub fn rungs(&self) -> RungTally {
        self.rungs
    }

    /// Whether the window is currently presenting.
    #[must_use]
    pub fn is_presenting(&self) -> bool {
        self.missed_in_a_row == 0
    }

    /// Record a frame that reached the screen: the ladder is done.
    pub fn presented(&mut self) {
        self.missed_in_a_row = 0;
        self.broken_in_a_row = 0;
        self.last_missed = None;
        self.last_rung = None;
    }

    /// Record a frame that did not reach the screen, and answer which rung
    /// of the recovery ladder it earns.
    ///
    /// `None` for a wait: the frame is counted, but nothing is rebuilt,
    /// because nothing is broken.
    pub fn missed(&mut self, missed: Missed) -> Option<Rung> {
        self.missed_in_a_row = self.missed_in_a_row.saturating_add(1);
        self.misses.note(missed);
        self.last_missed = Some(missed);
        if !missed.is_invalidation() {
            return None;
        }
        self.broken_in_a_row = self.broken_in_a_row.saturating_add(1);
        let rung = match self.broken_in_a_row {
            1 => Rung::Reconfigured,
            2 => Rung::Rebuilt,
            _ => Rung::Repeated,
        };
        self.rungs.note(rung);
        self.last_rung = Some(rung);
        Some(rung)
    }
}

#[cfg(test)]
mod tests {
    use super::{DeviceLiveness, Missed, Rung, SurfaceHealth};

    #[test]
    fn a_lost_device_is_one_fact_seen_through_every_handle() {
        // ★ R2088.1 — the load-bearing property, and the reason this is a
        // shared handle rather than a `Copy` flag: `wgpu`'s device-lost
        // callback holds one clone and every surface on that device holds
        // another. A value type here would leave the surfaces reading a
        // liveness that never changes, which is indistinguishable from a
        // healthy device and is exactly the silence this type exists to end.
        let held_by_the_callback = DeviceLiveness::default();
        let held_by_a_surface = held_by_the_callback.clone();
        assert!(!held_by_the_callback.is_lost());
        assert!(!held_by_a_surface.is_lost());
        held_by_the_callback.lose();
        assert!(
            held_by_a_surface.is_lost(),
            "the callback's handle and the surface's handle must be one fact"
        );
    }

    /// R2088.1 — the COMPILER's check that `GpuSurface::configure` opens a
    /// scope for every filter an error scope can be opened for.
    ///
    /// The failing path is the BUILD, not an assertion: this match has no
    /// wildcard, so a `wgpu` that grows a fourth `ErrorFilter` stops
    /// compiling here rather than letting that kind escape to the uncaptured
    /// handler — which is this round's own defect, one layer up.
    #[allow(dead_code)]
    fn every_error_filter_has_a_scope(filter: wgpu::ErrorFilter) -> &'static str {
        match filter {
            wgpu::ErrorFilter::OutOfMemory => "out-of-memory",
            wgpu::ErrorFilter::Validation => "validation",
            wgpu::ErrorFilter::Internal => "internal",
        }
    }

    #[test]
    fn a_fresh_surface_is_presenting_and_owes_no_recovery() {
        let health = SurfaceHealth::default();
        assert!(health.is_presenting());
        assert_eq!(health.missed_in_a_row(), 0);
        assert_eq!(health.last_missed(), None);
        assert_eq!(health.last_rung(), None);
        assert_eq!(health.rebuilds(), 0);
    }

    #[test]
    fn the_ladder_is_climbed_one_rung_per_consecutive_breakage() {
        let mut health = SurfaceHealth::default();
        assert_eq!(health.missed(Missed::Outdated), Some(Rung::Reconfigured));
        assert_eq!(health.missed(Missed::Outdated), Some(Rung::Rebuilt));
        assert_eq!(health.missed(Missed::Outdated), Some(Rung::Repeated));
        assert_eq!(health.missed(Missed::Outdated), Some(Rung::Repeated));
        assert_eq!(health.missed_in_a_row(), 4);
        assert_eq!(health.broken_in_a_row(), 4);
    }

    #[test]
    fn a_frame_that_presents_returns_the_ladder_to_the_bottom() {
        let mut health = SurfaceHealth::default();
        assert_eq!(health.missed(Missed::Outdated), Some(Rung::Reconfigured));
        health.presented();
        assert!(health.is_presenting());
        assert_eq!(health.last_missed(), None);
        assert_eq!(health.last_rung(), None);
        // ★ The point of the reset: the NEXT outage starts cheap again. A
        // ladder that stayed where it was would rebuild the surface on
        // every resize for the rest of the window's life.
        assert_eq!(health.missed(Missed::Outdated), Some(Rung::Reconfigured));
    }

    #[test]
    fn waiting_is_counted_and_takes_no_rung() {
        let mut health = SurfaceHealth::default();
        assert_eq!(health.missed(Missed::Occluded), None);
        assert_eq!(health.missed(Missed::Timeout), None);
        assert_eq!(health.missed_in_a_row(), 2);
        assert_eq!(health.broken_in_a_row(), 0);
        assert!(!health.is_presenting());
        assert_eq!(health.last_missed(), Some(Missed::Timeout));
        assert_eq!(health.last_rung(), None);
    }

    #[test]
    fn a_wait_before_a_breakage_does_not_spend_the_cheap_rung() {
        // The reason the two counters are separate. Five occluded frames
        // then a real invalidation must still try the response that fixes
        // the ordinary case first.
        let mut health = SurfaceHealth::default();
        for _ in 0..5 {
            assert_eq!(health.missed(Missed::Occluded), None);
        }
        assert_eq!(health.missed(Missed::Outdated), Some(Rung::Reconfigured));
        assert_eq!(health.missed_in_a_row(), 6);
        assert_eq!(health.broken_in_a_row(), 1);
    }

    #[test]
    fn rebuilds_are_cumulative_across_outages() {
        let mut health = SurfaceHealth::default();
        for _ in 0..3 {
            health.missed(Missed::Outdated);
            health.missed(Missed::Outdated);
            assert_eq!(health.last_rung(), Some(Rung::Rebuilt));
            health.presented();
        }
        assert_eq!(health.rebuilds(), 3);
        // ...and survive recovery, which is the whole reason they are not
        // cleared by `presented`.
        assert!(health.is_presenting());
    }

    #[test]
    fn the_heavy_rung_is_taken_once_per_outage_however_long_it_lasts() {
        let mut health = SurfaceHealth::default();
        for _ in 0..20 {
            health.missed(Missed::Outdated);
        }
        assert_eq!(health.rebuilds(), 1);
        assert_eq!(health.last_rung(), Some(Rung::Repeated));
    }

    #[test]
    fn every_status_is_classified_as_breakage_or_wait() {
        // A census over the type's own roster, so a new arm cannot be added
        // without deciding which side of the ladder it falls on.
        let breakages: Vec<Missed> = Missed::ALL
            .into_iter()
            .filter(|m| m.is_invalidation())
            .collect();
        let waits: Vec<Missed> = Missed::ALL
            .into_iter()
            .filter(|m| !m.is_invalidation())
            .collect();
        assert_eq!(breakages.len() + waits.len(), Missed::ALL.len());
        assert!(!breakages.is_empty() && !waits.is_empty());
    }

    #[test]
    fn every_unit_status_maps_to_its_own_miss() {
        // The five non-presentable statuses `wgpu` spells as unit variants can
        // be constructed here, so the mapping the render path and the capture
        // path share is checked rather than assumed. (`Success` / `Suboptimal`
        // carry a texture that only a real device can produce, which is why
        // R2088 made `split` hand it back instead of leaving each caller to
        // re-destructure the enum for it.)
        for (status, expected) in [
            (wgpu::CurrentSurfaceTexture::Timeout, Missed::Timeout),
            (wgpu::CurrentSurfaceTexture::Occluded, Missed::Occluded),
            (wgpu::CurrentSurfaceTexture::Outdated, Missed::Outdated),
            (wgpu::CurrentSurfaceTexture::Lost, Missed::Lost),
            (wgpu::CurrentSurfaceTexture::Validation, Missed::Validation),
        ] {
            let named = format!("{status:?}");
            assert_eq!(Missed::split(status).err(), Some(expected), "{named}");
        }
    }

    #[test]
    fn the_arm_pinion_raises_itself_is_not_one_wgpu_can_answer() {
        // R2088 — `Unconfigured` exists precisely because the status enum
        // cannot carry it: reaching `get_current_texture()` on a surface that
        // was never configured is fatal, so the answer has to be given before
        // the call. Nothing `split` can be handed may produce it.
        for status in [
            wgpu::CurrentSurfaceTexture::Timeout,
            wgpu::CurrentSurfaceTexture::Occluded,
            wgpu::CurrentSurfaceTexture::Outdated,
            wgpu::CurrentSurfaceTexture::Lost,
            wgpu::CurrentSurfaceTexture::Validation,
        ] {
            assert_ne!(Missed::split(status).err(), Some(Missed::Unconfigured));
        }
        // And it is a breakage, not a wait: a surface in this state will not
        // present again until a rung of the ladder puts it back.
        assert!(Missed::Unconfigured.is_invalidation());
        assert_eq!(
            SurfaceHealth::default().missed(Missed::Unconfigured),
            Some(Rung::Reconfigured)
        );
    }

    #[test]
    fn the_wire_spellings_are_what_clients_read() {
        // Pinned, because these strings leave the process: they are what
        // `scene/render_fidelity` publishes, and a rename here is a wire
        // break that no other test in this crate would feel.
        assert_eq!(Missed::Outdated.as_str(), "outdated");
        assert_eq!(Missed::Lost.as_str(), "lost");
        assert_eq!(Missed::Validation.as_str(), "validation");
        assert_eq!(Missed::Timeout.as_str(), "timeout");
        assert_eq!(Missed::Occluded.as_str(), "occluded");
        assert_eq!(Missed::Unconfigured.as_str(), "unconfigured");
        assert_eq!(Missed::DeviceLost.as_str(), "device_lost");
        assert_eq!(Rung::Reconfigured.as_str(), "reconfigured");
        assert_eq!(Rung::Rebuilt.as_str(), "rebuilt");
        assert_eq!(Rung::Repeated.as_str(), "repeated");
    }

    #[test]
    fn the_wire_spellings_are_distinct() {
        let missed: std::collections::BTreeSet<&str> =
            Missed::ALL.into_iter().map(Missed::as_str).collect();
        assert_eq!(missed.len(), Missed::ALL.len());
        let rungs: std::collections::BTreeSet<&str> =
            Rung::ALL.into_iter().map(Rung::as_str).collect();
        assert_eq!(rungs.len(), Rung::ALL.len());
    }

    #[test]
    fn every_reason_and_rung_indexes_back_to_itself() {
        // R2090 — the tallies are arrays keyed by a `slot()` match, and the
        // rows are derived from `ALL`. Two declarations of one order, so
        // this is what keeps them from drifting: the compiler makes `slot`
        // exhaustive, and this makes it AGREE with `ALL`. A duplicated or
        // transposed slot silently merges two reasons' counts, which is the
        // failure a tally cannot report about itself.
        for missed in Missed::ALL {
            assert_eq!(
                Missed::ALL[missed.slot()],
                missed,
                "{missed} does not index back to itself"
            );
        }
        for rung in Rung::ALL {
            assert_eq!(
                Rung::ALL[rung.slot()],
                rung,
                "{rung} does not index back to itself"
            );
        }
    }

    #[test]
    fn a_window_that_recovered_still_says_what_it_went_through() {
        // ★★★★★ R2090 — the property the whole tally exists for, and the one
        // this type could not answer before: after a recovery every "in a
        // row" counter is 0, so a reader arriving afterwards saw a window
        // that looked as if nothing had ever happened to it. Measured on
        // this host, a churn probe drove 40 tear-off generations and the
        // only cumulative field (the rebuild count) stayed 0 through runs
        // where frames HAD been missed — because they recovered on the
        // cheap rung, which nothing counted.
        let mut health = SurfaceHealth::default();
        health.missed(Missed::Outdated);
        health.missed(Missed::Occluded);
        health.presented();

        assert_eq!(health.missed_in_a_row(), 0, "the window is presenting");
        assert_eq!(health.broken_in_a_row(), 0);
        assert_eq!(health.last_missed(), None);
        assert!(health.is_presenting());

        assert_eq!(health.misses().total(), 2, "and it still remembers both");
        assert_eq!(health.misses().of(Missed::Outdated), 1);
        assert_eq!(health.misses().of(Missed::Occluded), 1);
        assert_eq!(health.misses().of(Missed::DeviceLost), 0);
        assert_eq!(
            health.rungs().of(Rung::Reconfigured),
            1,
            "the cheap rung was taken once, and is now counted"
        );
        assert_eq!(health.rungs().of(Rung::Rebuilt), 0);
    }

    #[test]
    fn a_wait_is_tallied_but_earns_no_rung() {
        // The two counters answer different questions (this module's header
        // says so), and the tallies must keep that separation: an occluded
        // window has missed frames a viewer did not see, and nothing about
        // it is broken.
        let mut health = SurfaceHealth::default();
        health.missed(Missed::Occluded);
        health.missed(Missed::Timeout);
        assert_eq!(health.misses().total(), 2);
        assert_eq!(
            health.rungs().rows().iter().map(|(_, n)| *n).sum::<u32>(),
            0,
            "waiting is not evidence that anything needs rebuilding"
        );
    }

    #[test]
    fn the_rebuild_count_is_the_heavy_rungs_row() {
        // R2090 — `rebuilds()` used to be its own field incremented beside
        // the rung it names: one fact with two writers, which can disagree.
        // It is now derived, and this walks the ladder far enough to reach
        // the heavy rung twice over two breakages.
        let mut health = SurfaceHealth::default();
        for _ in 0..3 {
            health.missed(Missed::Lost);
        }
        assert_eq!(health.last_rung(), Some(Rung::Repeated));
        assert_eq!(health.rebuilds(), health.rungs().of(Rung::Rebuilt));
        assert_eq!(health.rebuilds(), 1, "the heavy rung was reached once");
        assert_eq!(health.rungs().of(Rung::Reconfigured), 1);
        assert_eq!(health.rungs().of(Rung::Repeated), 1);

        health.presented();
        for _ in 0..2 {
            health.missed(Missed::DeviceLost);
        }
        assert_eq!(
            health.rebuilds(),
            2,
            "a second breakage climbs to the heavy rung again"
        );
        assert_eq!(health.misses().of(Missed::DeviceLost), 2);
        assert_eq!(health.misses().of(Missed::Lost), 3, "and the first is kept");
        assert_eq!(health.misses().total(), 5);
    }

    #[test]
    fn the_rows_are_derived_from_the_vocabulary() {
        // A publisher writes the rows out; deriving them here is what keeps
        // a consumer from spelling the vocabulary a second time (the class
        // this repository has paid for at every census).
        let mut health = SurfaceHealth::default();
        health.missed(Missed::Unconfigured);
        let rows = health.misses().rows();
        assert_eq!(rows.len(), Missed::ALL.len());
        for (i, (missed, count)) in rows.into_iter().enumerate() {
            assert_eq!(missed, Missed::ALL[i], "rows follow ALL order");
            assert_eq!(count, health.misses().of(missed));
        }
        assert_eq!(health.rungs().rows().len(), Rung::ALL.len());
    }
}

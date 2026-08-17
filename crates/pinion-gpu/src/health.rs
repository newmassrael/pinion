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
}

impl Missed {
    /// Every arm, so a census or a doc table derives its rows instead of
    /// hand-listing them.
    pub const ALL: [Self; 5] = [
        Self::Outdated,
        Self::Lost,
        Self::Validation,
        Self::Timeout,
        Self::Occluded,
    ];

    /// Whether this is the surface *breaking* — the case a recovery can act
    /// on — rather than the window waiting.
    ///
    /// This is the discrimination the recovery ladder is built on: waiting
    /// is not evidence that anything needs rebuilding, and treating it as
    /// such would spend the ladder's rungs on a window that is fine.
    #[must_use]
    pub fn is_invalidation(self) -> bool {
        match self {
            Self::Outdated | Self::Lost | Self::Validation => true,
            Self::Timeout | Self::Occluded => false,
        }
    }

    /// The miss a non-presentable acquisition names, or `None` when the
    /// acquisition handed over an image.
    ///
    /// Lives here so the mapping is written once. It had been written
    /// twice — the emitted renderer's `render` and the shell's screenshot
    /// capture each spelled all six arms out — which is two chances for a
    /// status to be classified differently on the two paths a frame can
    /// take to the same screen.
    #[must_use]
    pub fn of(status: &wgpu::CurrentSurfaceTexture) -> Option<Self> {
        match status {
            wgpu::CurrentSurfaceTexture::Success(_)
            | wgpu::CurrentSurfaceTexture::Suboptimal(_) => None,
            wgpu::CurrentSurfaceTexture::Timeout => Some(Self::Timeout),
            wgpu::CurrentSurfaceTexture::Occluded => Some(Self::Occluded),
            wgpu::CurrentSurfaceTexture::Outdated => Some(Self::Outdated),
            wgpu::CurrentSurfaceTexture::Lost => Some(Self::Lost),
            wgpu::CurrentSurfaceTexture::Validation => Some(Self::Validation),
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
        }
    }
}

impl core::fmt::Display for Missed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
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
    rebuilds: u32,
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
    /// there is a different fact from one that has needed none, and it is
    /// the only fact that survives to say the heavy rung is load-bearing on
    /// this host.
    #[must_use]
    pub fn rebuilds(&self) -> u32 {
        self.rebuilds
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
        if rung == Rung::Rebuilt {
            self.rebuilds = self.rebuilds.saturating_add(1);
        }
        self.last_rung = Some(rung);
        Some(rung)
    }
}

#[cfg(test)]
mod tests {
    use super::{Missed, Rung, SurfaceHealth};

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
        // carry a texture that only a real device can produce; their `None` is
        // covered by the two paths' own `unclassified` arm, which is
        // unreachable exactly because of this table.)
        for (status, expected) in [
            (wgpu::CurrentSurfaceTexture::Timeout, Missed::Timeout),
            (wgpu::CurrentSurfaceTexture::Occluded, Missed::Occluded),
            (wgpu::CurrentSurfaceTexture::Outdated, Missed::Outdated),
            (wgpu::CurrentSurfaceTexture::Lost, Missed::Lost),
            (wgpu::CurrentSurfaceTexture::Validation, Missed::Validation),
        ] {
            assert_eq!(Missed::of(&status), Some(expected), "{status:?}");
        }
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
}

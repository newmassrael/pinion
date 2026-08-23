//! R1789 §5.28 — an **authored track of timed entries**, and the query a
//! scrub cannot answer without one: *which entries did I just cross?*
//!
//! # What this is beside, and what none of them are
//!
//! [`TransportClock`](super::transport::TransportClock) is a **playhead** — it
//! knows where in a duration it is and nothing about what is there.
//! `pinion_chart::timeline` is a **reading lane** — it draws spans somebody
//! already has. [`animation`](crate::animation) interpolates between **two**
//! points. So a person who wants to say *stop that node at eight seconds* has a
//! clock to scrub, a picture to look at and a curve to follow, and nowhere to
//! put the eight seconds.
//!
//! This is the missing third thing: a sequence a person **authors**, and a
//! [`due`](Track::due) query that answers *what falls in the half-open window a
//! tick just advanced through*. That query is the whole point — an
//! interpolating animation can tell you a VALUE at a time, and cannot tell you
//! which discrete events a step passed, which is exactly what a scenario is
//! made of.
//!
//! # The floor, built and run at 6.11 rather than read
//!
//! The reference has a keyframe API, so by this project's rule a consumer for
//! one exists. Measured:
//!
//! | asked | what it does |
//! |---|---|
//! | two keys at one step | the second **silently replaces** the first |
//! | a step outside its range | **dropped**; the only signal is a line on stderr |
//! | two keys it cannot interpolate | the value in between is an **invalid** empty variant, with no reason and no state change |
//! | *which keys did t0→t1 cross?* | **there is no such query** — a scrub answers a value, never the entries passed |
//!
//! Every one of those is a silent loss of something a person typed. Here a
//! duplicate is [`Misplaced::Taken`] and not a replacement (with
//! [`replace`](Track::replace) for when replacing IS the intent, so the two
//! cannot be confused), an unusable time is a **returned value** naming the
//! time rather than a line on a stream nobody reads, and `due` exists.
//!
//! # Times are seconds, and a duration is derived
//!
//! Not a `0.0..=1.0` fraction, which is the floor's choice and forces every
//! edit to be re-scaled when the scenario grows. And [`Track::duration`] is
//! read off the last entry rather than declared, so a track and its length
//! cannot disagree — the floor lets you set a duration shorter than the keys
//! you placed, and then quietly never reaches them.

use core::fmt;

/// One authored entry: when, and what is meant to happen then.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key<T> {
    at: Seconds,
    value: T,
}

impl<T> Key<T> {
    /// When this entry happens, in seconds from the track's start.
    #[must_use]
    pub const fn at(&self) -> Seconds {
        self.at
    }

    /// What is meant to happen.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }
}

/// A moment on a track: a finite, non-negative number of seconds.
///
/// A type rather than a bare `f32` because the two ways a time can be unusable
/// — not a number, and before the start — are the two refusals every edit here
/// has to be able to give, and a validated value means they are given **once**,
/// at the boundary, instead of at each of the four verbs.
///
/// `Eq` and `Ord` are derived from the bit pattern of a value that is already
/// known finite, which is what makes a track sortable and a time usable as a
/// key. A NaN never gets in, so the usual objection does not apply.
#[derive(Clone, Copy, Debug)]
pub struct Seconds(f32);

impl Seconds {
    /// The start of a track.
    pub const ZERO: Self = Self(0.0);

    /// `secs` as a moment, or why it is not one.
    ///
    /// # Errors
    /// [`Misplaced::NotATime`] for a NaN or an infinity — a value that cannot be
    /// ordered against another, so a track holding one could not be walked.
    /// [`Misplaced::BeforeStart`] for a negative, because a track starts when it
    /// starts and "two seconds before the beginning" is a request nobody can
    /// carry out.
    pub fn new(secs: f32) -> Result<Self, Misplaced> {
        if !secs.is_finite() {
            return Err(Misplaced::NotATime { given: secs });
        }
        if secs < 0.0 {
            return Err(Misplaced::BeforeStart { given: secs });
        }
        Ok(Self(secs))
    }

    /// This moment as a number of seconds.
    #[must_use]
    pub const fn secs(self) -> f32 {
        self.0
    }
}

impl PartialEq for Seconds {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Seconds {}

impl PartialOrd for Seconds {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Seconds {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Both are finite by construction, so this total order is real.
        self.0
            .partial_cmp(&other.0)
            .unwrap_or(core::cmp::Ordering::Equal)
    }
}

impl fmt::Display for Seconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}s", self.0)
    }
}

/// Why an entry could not go where it was asked to go.
///
/// A returned value, which is the difference from the floor: there a bad time
/// prints on stderr and the call returns void, so the caller's next read finds
/// the entry missing with nothing to say about it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Misplaced {
    /// The time was a NaN or an infinity.
    NotATime {
        /// What was given.
        given: f32,
    },
    /// The time was before the track starts.
    BeforeStart {
        /// What was given.
        given: f32,
    },
    /// Another entry is already at that moment.
    ///
    /// ★ The floor replaces it and says nothing. Replacing is a legitimate
    /// intent and has its own verb here ([`Track::replace`]); what is refused is
    /// doing it **by accident**, which is what a `place` that silently
    /// overwrites is.
    Taken {
        /// The moment already spoken for.
        at: Seconds,
    },
    /// There is no entry at that moment to move or remove.
    Empty {
        /// The moment that was asked about.
        at: Seconds,
    },
}

impl Misplaced {
    /// The wire name of this refusal kind.
    #[must_use]
    pub const fn as_wire_name(self) -> &'static str {
        match self {
            Self::NotATime { .. } => "not_a_time",
            Self::BeforeStart { .. } => "before_start",
            Self::Taken { .. } => "taken",
            Self::Empty { .. } => "empty",
        }
    }
}

impl fmt::Display for Misplaced {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotATime { given } => {
                write!(f, "{given} is not a moment a track can hold")
            }
            Self::BeforeStart { given } => write!(
                f,
                "{given}s is before the track starts, and a track starts when it starts"
            ),
            Self::Taken { at } => write!(
                f,
                "something already happens at {at}; move it, or replace it on purpose"
            ),
            Self::Empty { at } => write!(f, "nothing happens at {at}"),
        }
    }
}

impl std::error::Error for Misplaced {}

/// An authored sequence of timed entries, in time order.
///
/// One lane. Concurrency is several tracks — see [`Schedule`], which is what
/// "sequential and concurrent" needs and what a single ordered sequence cannot
/// express without inventing a meaning for two entries at one moment.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Track<T> {
    keys: Vec<Key<T>>,
}

impl<T> Default for Track<T> {
    fn default() -> Self {
        Self { keys: Vec::new() }
    }
}

impl<T> Track<T> {
    /// An empty track.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The entries, in time order.
    #[must_use]
    pub fn keys(&self) -> &[Key<T>] {
        &self.keys
    }

    /// How many entries there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether the track holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// How long this track is: the moment of its last entry, or zero.
    ///
    /// **Derived, never declared.** The floor keeps a duration beside the keys
    /// and lets the two disagree — a duration shorter than the last key means an
    /// animation that quietly never reaches it.
    #[must_use]
    pub fn duration(&self) -> Seconds {
        self.keys.last().map_or(Seconds::ZERO, |key| key.at)
    }

    /// The entry at exactly `at`, if there is one.
    #[must_use]
    pub fn at(&self, at: Seconds) -> Option<&Key<T>> {
        self.index_of(at).map(|i| &self.keys[i])
    }

    /// ★★★★★ The entries in the half-open window `(after, upto]` — **what a
    /// tick from `after` to `upto` just crossed.**
    ///
    /// The query the floor has no equivalent of, and the reason a scenario needs
    /// a track rather than an animation: an interpolating keyframe API answers
    /// *a value at a time* and can say nothing about which discrete entries a
    /// step passed, so "stop that node at eight seconds" is inexpressible there.
    ///
    /// **Half-open on purpose.** A driver ticks `(t, t+dt]` repeatedly, and each
    /// entry has to be delivered exactly once across the whole run: closed at
    /// both ends double-delivers the boundary, open at both drops it. An entry
    /// at exactly zero is therefore reached by a window that starts *below* it —
    /// which [`from_start`](Self::from_start) is for, so a caller never has to
    /// invent a negative time to get it.
    ///
    /// Empty when `upto <= after`, including when they are equal: no time
    /// passed, so nothing was crossed.
    #[must_use]
    pub fn due(&self, after: Seconds, upto: Seconds) -> &[Key<T>] {
        if upto <= after {
            return &[];
        }
        let start = self.keys.partition_point(|key| key.at <= after);
        let end = self.keys.partition_point(|key| key.at <= upto);
        &self.keys[start..end]
    }

    /// The entries from the very beginning up to and including `upto`.
    ///
    /// The window `due` cannot be given directly, because its lower bound is
    /// exclusive and there is no moment before zero. A run's first tick uses
    /// this and every later one uses [`due`](Self::due).
    #[must_use]
    pub fn from_start(&self, upto: Seconds) -> &[Key<T>] {
        &self.keys[..self.keys.partition_point(|key| key.at <= upto)]
    }

    /// Where `at` sits in the entry list, if an entry is there.
    fn index_of(&self, at: Seconds) -> Option<usize> {
        self.keys.binary_search_by(|key| key.at.cmp(&at)).ok()
    }
}

impl<T> Track<T> {
    /// Put `value` at `at`.
    ///
    /// # Errors
    /// [`Misplaced::Taken`] when something is already there — refused rather
    /// than overwritten, which is the floor's behaviour and the one that loses
    /// what a person typed. Use [`replace`](Self::replace) to mean it.
    /// [`Misplaced::NotATime`] / [`Misplaced::BeforeStart`] for a time no track
    /// can hold.
    pub fn place(&mut self, at: f32, value: T) -> Result<Seconds, Misplaced> {
        let at = Seconds::new(at)?;
        match self.keys.binary_search_by(|key| key.at.cmp(&at)) {
            Ok(_) => Err(Misplaced::Taken { at }),
            Err(index) => {
                self.keys.insert(index, Key { at, value });
                Ok(at)
            }
        }
    }

    /// Put `value` at `at`, replacing whatever was there.
    ///
    /// Answers what it displaced, so the caller can report it. The floor does
    /// this on every `place` and answers nothing.
    ///
    /// # Errors
    /// [`Misplaced::NotATime`] / [`Misplaced::BeforeStart`].
    pub fn replace(&mut self, at: f32, value: T) -> Result<Option<T>, Misplaced> {
        let at = Seconds::new(at)?;
        match self.keys.binary_search_by(|key| key.at.cmp(&at)) {
            Ok(index) => Ok(Some(core::mem::replace(&mut self.keys[index].value, value))),
            Err(index) => {
                self.keys.insert(index, Key { at, value });
                Ok(None)
            }
        }
    }

    /// Move the entry at `from` to `to`, keeping the track ordered.
    ///
    /// # Errors
    /// [`Misplaced::Empty`] when there is nothing at `from`,
    /// [`Misplaced::Taken`] when `to` is spoken for, and the time refusals for
    /// either end. Moving an entry onto its own moment succeeds and changes
    /// nothing — it is a request that is already satisfied, not a collision.
    pub fn shift(&mut self, from: f32, to: f32) -> Result<Seconds, Misplaced> {
        let from = Seconds::new(from)?;
        let to = Seconds::new(to)?;
        let Some(index) = self.index_of(from) else {
            return Err(Misplaced::Empty { at: from });
        };
        if from == to {
            return Ok(to);
        }
        if self.index_of(to).is_some() {
            return Err(Misplaced::Taken { at: to });
        }
        let mut key = self.keys.remove(index);
        key.at = to;
        let index = self.keys.partition_point(|other| other.at < to);
        self.keys.insert(index, key);
        Ok(to)
    }

    /// Take the entry at `at` off the track.
    ///
    /// # Errors
    /// [`Misplaced::Empty`] when nothing is there, and the time refusals.
    pub fn remove(&mut self, at: f32) -> Result<T, Misplaced> {
        let at = Seconds::new(at)?;
        let Some(index) = self.index_of(at) else {
            return Err(Misplaced::Empty { at });
        };
        Ok(self.keys.remove(index).value)
    }
}

/// Several named tracks, read together.
///
/// What "sequential and concurrent" needs: two things happening at one moment
/// are two entries on two tracks, which is a fact a single ordered sequence
/// cannot hold without deciding what a duplicate time means — and deciding that
/// is how a track ends up silently replacing an entry, which is the floor.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Schedule<T> {
    tracks: Vec<(String, Track<T>)>,
}

impl<T> Default for Schedule<T> {
    fn default() -> Self {
        Self { tracks: Vec::new() }
    }
}

impl<T> Schedule<T> {
    /// An empty schedule.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The track called `name`, adding an empty one if there is none.
    ///
    /// Named lanes are created by being written to, because a schedule's lanes
    /// are whatever the scenario turned out to need and asking a caller to
    /// declare them first is a second list to keep in step.
    pub fn track(&mut self, name: &str) -> &mut Track<T> {
        if let Some(index) = self.tracks.iter().position(|(had, _)| had == name) {
            return &mut self.tracks[index].1;
        }
        self.tracks.push((name.to_owned(), Track::new()));
        let last = self.tracks.len() - 1;
        &mut self.tracks[last].1
    }

    /// The track called `name`, if it exists.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Track<T>> {
        self.tracks
            .iter()
            .find(|(had, _)| had == name)
            .map(|(_, track)| track)
    }

    /// The lane names, in the order they were first written to.
    #[must_use]
    pub fn lanes(&self) -> Vec<&str> {
        self.tracks.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// How long the schedule is: the furthest moment any lane reaches.
    #[must_use]
    pub fn duration(&self) -> Seconds {
        self.tracks
            .iter()
            .map(|(_, track)| track.duration())
            .max()
            .unwrap_or(Seconds::ZERO)
    }

    /// Every entry any lane holds in `(after, upto]`, **in time order across
    /// lanes**, each with the lane it is on.
    ///
    /// Time order and not lane order: a run consuming this does things in the
    /// order they happen, and two entries at one moment are concurrent by
    /// definition — their relative order is then the lane order, which is
    /// stable and stated rather than arbitrary.
    #[must_use]
    pub fn due(&self, after: Seconds, upto: Seconds) -> Vec<(&str, &Key<T>)> {
        self.gather(|track| track.due(after, upto))
    }

    /// Every entry any lane holds from the very beginning up to `upto`.
    #[must_use]
    pub fn from_start(&self, upto: Seconds) -> Vec<(&str, &Key<T>)> {
        self.gather(|track| track.from_start(upto))
    }

    /// Collect a per-track window across every lane, in time order.
    fn gather<'a>(
        &'a self,
        window: impl Fn(&'a Track<T>) -> &'a [Key<T>],
    ) -> Vec<(&'a str, &'a Key<T>)> {
        let mut out: Vec<(&str, &Key<T>)> = self
            .tracks
            .iter()
            .flat_map(|(name, track)| window(track).iter().map(move |key| (name.as_str(), key)))
            .collect();
        // A stable sort by time alone, so entries sharing a moment keep the
        // lane order they were gathered in.
        out.sort_by(|(_, a), (_, b)| a.at.cmp(&b.at));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(v: f32) -> Seconds {
        Seconds::new(v).expect("a usable moment")
    }

    fn track() -> Track<&'static str> {
        let mut track = Track::new();
        track.place(0.0, "warmup").unwrap();
        track.place(8.0, "kill n1").unwrap();
        track.place(2.5, "send").unwrap();
        track
    }

    #[test]
    fn r1789_entries_are_in_time_order_however_they_were_placed() {
        let track = track();
        assert_eq!(
            track
                .keys()
                .iter()
                .map(Key::value)
                .copied()
                .collect::<Vec<_>>(),
            vec!["warmup", "send", "kill n1"]
        );
        assert_eq!(track.duration(), secs(8.0), "derived from the last entry");
    }

    #[test]
    fn r1789_a_second_entry_at_one_moment_is_refused_not_swallowed() {
        let mut track = track();
        // The floor replaces silently. Here it is a value, and it names when.
        assert_eq!(
            track.place(2.5, "other"),
            Err(Misplaced::Taken { at: secs(2.5) })
        );
        assert_eq!(track.len(), 3, "and nothing was displaced");
        assert_eq!(track.at(secs(2.5)).map(Key::value), Some(&"send"));
        // Replacing on purpose works and SAYS what it displaced, which the
        // floor's silent replace cannot.
        assert_eq!(track.replace(2.5, "other"), Ok(Some("send")));
        assert_eq!(track.at(secs(2.5)).map(Key::value), Some(&"other"));
        assert_eq!(track.len(), 3);
        // Replacing where nothing was is a plain placement, and says so.
        assert_eq!(track.replace(3.0, "new"), Ok(None));
        assert_eq!(track.len(), 4);
    }

    #[test]
    fn r1789_a_time_no_track_can_hold_is_a_returned_value() {
        let mut track = Track::new();
        // Compared by wire name, not by value: a NaN is not equal to itself, so
        // asserting `Err(NotATime { given: NAN })` would pass for any refusal
        // carrying a NaN and fail for the right one. The reason the refusal
        // exists is the reason it cannot be compared.
        assert_eq!(
            track.place(f32::NAN, "x").map_err(Misplaced::as_wire_name),
            Err("not_a_time")
        );
        assert_eq!(
            track
                .place(f32::INFINITY, "x")
                .map_err(Misplaced::as_wire_name),
            Err("not_a_time")
        );
        assert_eq!(
            track
                .place(f32::NEG_INFINITY, "x")
                .map_err(Misplaced::as_wire_name),
            Err("not_a_time"),
            "★ a negative infinity is not-a-time and not before-the-start — the \
             order of the two checks is a decision, and this is it"
        );
        assert_eq!(
            track.place(-1.0, "x"),
            Err(Misplaced::BeforeStart { given: -1.0 })
        );
        assert!(track.is_empty(), "not one of them landed");
        // And each says something a person can act on.
        for why in [
            Misplaced::BeforeStart { given: -1.0 },
            Misplaced::Taken { at: secs(1.0) },
            Misplaced::Empty { at: secs(1.0) },
        ] {
            assert!(!why.to_string().is_empty(), "{why:?} says nothing");
        }
    }

    #[test]
    fn r1789_a_window_delivers_every_entry_exactly_once_across_a_run() {
        let track = track();
        // Tick a whole run in steps and collect what each window crossed. The
        // half-open rule is what makes this a partition rather than a guess.
        let mut seen: Vec<&str> = track
            .from_start(secs(1.0))
            .iter()
            .map(|k| *k.value())
            .collect();
        let mut at = 1.0_f32;
        while at < 10.0 {
            let next = at + 1.0;
            seen.extend(track.due(secs(at), secs(next)).iter().map(|k| *k.value()));
            at = next;
        }
        assert_eq!(
            seen,
            vec!["warmup", "send", "kill n1"],
            "every entry once, in order, with none repeated at a boundary"
        );
    }

    #[test]
    fn r1789_a_window_that_passes_no_time_crosses_nothing() {
        let track = track();
        assert!(track.due(secs(2.5), secs(2.5)).is_empty(), "same moment");
        assert!(track.due(secs(4.0), secs(2.0)).is_empty(), "backwards");
        // The boundary belongs to the window that ENDS on it, not the one that
        // starts there — which is what stops a double delivery.
        assert_eq!(track.due(secs(0.0), secs(2.5)).len(), 1);
        assert!(track.due(secs(2.5), secs(3.0)).is_empty());
    }

    #[test]
    fn r1789_the_first_entry_needs_no_negative_time_to_be_reached() {
        let track = track();
        // `due` is open below, so nothing reaches an entry at zero. That is why
        // `from_start` exists rather than leaving a caller to pass -0.001.
        assert!(track.due(secs(0.0), secs(1.0)).is_empty());
        assert_eq!(track.from_start(secs(1.0)).len(), 1);
        assert_eq!(track.from_start(secs(0.0)).len(), 1, "zero is included");
    }

    #[test]
    fn r1789_moving_an_entry_keeps_the_track_ordered_and_refuses_a_collision() {
        let mut track = track();
        assert_eq!(track.shift(0.0, 5.0), Ok(secs(5.0)));
        assert_eq!(
            track
                .keys()
                .iter()
                .map(Key::value)
                .copied()
                .collect::<Vec<_>>(),
            vec!["send", "warmup", "kill n1"],
            "it moved past its neighbour and the order followed"
        );
        assert_eq!(
            track.shift(5.0, 8.0),
            Err(Misplaced::Taken { at: secs(8.0) })
        );
        assert_eq!(
            track.at(secs(5.0)).map(Key::value),
            Some(&"warmup"),
            "unmoved"
        );
        assert_eq!(
            track.shift(5.0, 5.0),
            Ok(secs(5.0)),
            "onto itself is a no-op"
        );
        assert_eq!(
            track.shift(6.0, 7.0),
            Err(Misplaced::Empty { at: secs(6.0) })
        );
    }

    #[test]
    fn r1789_removing_says_what_it_took_and_refuses_what_is_not_there() {
        let mut track = track();
        assert_eq!(track.remove(2.5), Ok("send"));
        assert_eq!(track.len(), 2);
        assert_eq!(track.remove(2.5), Err(Misplaced::Empty { at: secs(2.5) }));
        assert_eq!(track.duration(), secs(8.0), "the last entry still ends it");
        track.remove(8.0).unwrap();
        assert_eq!(track.duration(), secs(0.0), "and now the first one does");
    }

    #[test]
    fn r1789_two_lanes_are_concurrent_and_a_run_reads_them_in_time_order() {
        let mut schedule = Schedule::new();
        schedule.track("traffic").place(1.0, "start").unwrap();
        schedule.track("faults").place(8.0, "kill n1").unwrap();
        schedule.track("traffic").place(8.0, "burst").unwrap();
        assert_eq!(schedule.lanes(), vec!["traffic", "faults"], "first-written");
        assert_eq!(schedule.duration(), secs(8.0), "the furthest lane");

        let due = schedule.due(secs(0.5), secs(9.0));
        assert_eq!(
            due.iter()
                .map(|(lane, k)| (*lane, *k.value()))
                .collect::<Vec<_>>(),
            vec![
                ("traffic", "start"),
                ("traffic", "burst"),
                ("faults", "kill n1")
            ],
            "★ in TIME order across lanes, and two at one moment keep lane order"
        );
        // A lane nobody wrote to is absent rather than empty, so `lanes()` is
        // what the scenario turned out to need.
        assert!(schedule.get("nothing").is_none());
        assert_eq!(schedule.get("faults").map(Track::len), Some(1));
    }

    #[test]
    fn r1789_a_schedule_window_is_the_union_of_its_lanes_windows() {
        let mut schedule = Schedule::new();
        schedule.track("a").place(0.0, "a0").unwrap();
        schedule.track("b").place(0.0, "b0").unwrap();
        schedule.track("a").place(4.0, "a4").unwrap();
        assert_eq!(schedule.from_start(secs(0.0)).len(), 2, "both zeroes");
        assert!(schedule.due(secs(0.0), secs(1.0)).is_empty(), "past them");
        assert_eq!(schedule.due(secs(1.0), secs(4.0)).len(), 1);
        assert!(schedule.due(secs(4.0), secs(4.0)).is_empty());
    }

    #[test]
    fn r1789_an_empty_track_and_an_empty_schedule_answer_rather_than_panic() {
        let track: Track<&str> = Track::new();
        assert!(track.is_empty());
        assert_eq!(track.duration(), Seconds::ZERO);
        assert!(track.due(secs(0.0), secs(100.0)).is_empty());
        assert!(track.from_start(secs(100.0)).is_empty());
        assert!(track.at(secs(0.0)).is_none());
        let schedule: Schedule<&str> = Schedule::new();
        assert!(schedule.lanes().is_empty());
        assert_eq!(schedule.duration(), Seconds::ZERO);
        assert!(schedule.due(secs(0.0), secs(100.0)).is_empty());
    }

    #[test]
    fn r1789_every_refusal_kind_has_a_distinct_wire_name() {
        let all = [
            Misplaced::NotATime { given: 0.0 },
            Misplaced::BeforeStart { given: 0.0 },
            Misplaced::Taken { at: Seconds::ZERO },
            Misplaced::Empty { at: Seconds::ZERO },
        ];
        let mut names: Vec<&str> = all.iter().map(|w| w.as_wire_name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len());
    }
}

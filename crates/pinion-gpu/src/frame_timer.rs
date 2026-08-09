//! R1537 §5.16 — two timestamps around one frame's GPU work.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Number of timestamps per frame: one before the first submission, one
/// after the blit.
const QUERY_COUNT: u32 = 2;

/// Bytes the resolved query set occupies — `wgpu` writes each timestamp as
/// a `u64`.
const RESOLVE_BYTES: wgpu::BufferAddress = QUERY_COUNT as wgpu::BufferAddress * 8;

/// Map-callback signal. An `AtomicU8` rather than a channel because the
/// callback may run on the polling thread inside `Device::poll` and must
/// not allocate or block; three states are all this needs.
const MAP_WAITING: u8 = 0;
const MAP_READY: u8 = 1;
const MAP_FAILED: u8 = 2;

/// What a backend can say about the GPU's own clock.
///
/// Three states rather than an `Option<u64>`, because an option has only
/// two and the frame needs three: *this host cannot ever time the GPU*,
/// *it can and has not yet*, and *here is the number*. Collapsing the
/// first two into one `None` makes them indistinguishable to a caller —
/// and they call for opposite responses, since one is a permanent
/// property of the machine and the other resolves in a frame or two.
///
/// It also makes the contradictory pair unrepresentable. With a separate
/// `supported: bool` beside an `Option<u64>`, `(false, Some(900))` is a
/// value the types allow and nothing means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuFrameClock {
    /// The device has no timestamp queries, so no frame on this host will
    /// ever be timed. A permanent property of the adapter, not a state.
    Unsupported,
    /// Timing is running; no measurement has been harvested yet. Expected
    /// for the first frames of a window, since a timestamp is only
    /// readable after the GPU has executed the commands that wrote it.
    Pending,
    /// GPU wall-clock microseconds for a recent frame.
    Measured(u64),
}

impl GpuFrameClock {
    /// The measured value, if there is one. `None` for both
    /// [`Self::Unsupported`] and [`Self::Pending`] — use the variant
    /// itself when the difference matters.
    #[must_use]
    pub fn measured(self) -> Option<u64> {
        match self {
            Self::Measured(us) => Some(us),
            Self::Unsupported | Self::Pending => None,
        }
    }

    /// Whether this host can time the GPU at all — i.e. whether waiting
    /// longer could ever turn this into a [`Self::Measured`].
    #[must_use]
    pub fn is_supported(self) -> bool {
        !matches!(self, Self::Unsupported)
    }
}

/// Times one frame's GPU execution with a pair of `wgpu` timestamp
/// queries, and reads the result back **without ever blocking the frame
/// that wrote it**.
///
/// # What is being measured
///
/// GPU wall-clock from the start of the frame's first submission to the
/// end of its blit — that is, the rasterizer's compute passes *plus* the
/// copy to the presented image. This is the number
/// `pinion_runtime::FrameTiming::render_us`
/// is not: `render_us` measures the CPU recording and handing that work to
/// the driver, and `wgpu` returns from `submit` long before the GPU is
/// finished, so the two are unrelated quantities that a reader could
/// otherwise mistake for each other.
///
/// # Why the result is always one frame late
///
/// A timestamp is readable only after the GPU has executed the commands
/// that wrote it. Waiting for that inside the frame would serialise CPU
/// and GPU — the exact stall a profiler exists to find, introduced by the
/// profiler. So the read is *polled*: the frame that writes the queries
/// submits and moves on, and a later frame harvests the result when it
/// happens to be ready. A harvested reading therefore describes a recent
/// frame, not the one just painted. Every GPU profiler works this way
/// (the engine's `stat gpu` included); the alternative is not "fresher
/// numbers" but "different numbers", because the measurement would have
/// changed what it measured.
///
/// A frame whose result has not been harvested yet does not start a new
/// measurement — the query set and staging buffer are single-slot, and
/// overwriting them mid-flight would resolve a frame against another
/// frame's start. So under a backlog, samples are *skipped*, never
/// blended. A caller counts the ones that landed by counting the frame
/// samples that carry a reading, which is the same question asked where the
/// answer is already kept.
///
/// # Cost
///
/// One extra, near-empty command-buffer submission per timed frame. The
/// opening timestamp has to be ordered *before* the rasterizer's own
/// submission, and the rasterizer (`vello::Renderer::render_to_texture`)
/// submits internally rather than recording into a caller's encoder, so
/// there is no encoder in existence at that moment to append to. The
/// closing timestamp needs no such thing: it rides the blit encoder that
/// the frame already creates.
pub struct FrameTimer {
    query_set: wgpu::QuerySet,
    /// `QUERY_RESOLVE | COPY_SRC` — where `resolve_query_set` puts the raw
    /// ticks. Not mappable: a buffer cannot be both a resolve target and
    /// host-visible on every backend, which is why there are two.
    resolve: wgpu::Buffer,
    /// `MAP_READ | COPY_DST` — the host-visible copy.
    staging: wgpu::Buffer,
    /// Nanoseconds per tick, from the queue. A device constant.
    period_ns: f32,
    signal: Arc<AtomicU8>,
    /// The opening timestamp is recorded but the closing one is not yet.
    armed: bool,
    /// The closing timestamp and its resolve are recorded; the encoder
    /// carrying them still has to be submitted before a map can be asked
    /// for.
    awaiting_submit: bool,
    /// A map is outstanding: the buffers belong to a frame in flight and
    /// no new measurement may start.
    map_in_flight: bool,
    /// A measurement harvested since the last [`FrameTimer::clock`] call,
    /// waiting to be reported exactly once.
    ///
    /// A *delta*, not a level, and that is the whole point: reporting a
    /// persisting level once per frame would put the SAME measurement on
    /// every sample in the ring, which makes a mean over those samples a
    /// mean of one number repeated and makes "how many samples carry a
    /// timing" count frames instead of measurements.
    ///
    /// The only stored reading. An earlier draft also kept a persisting
    /// *level* beside it, which is two sources for one question and had no
    /// consumer: the windowed level a caller actually wants is the fold
    /// over the frame ring (`mean_gpu_us` / `max_gpu_us`), not a field
    /// here.
    fresh: Option<u64>,
    dropped: u64,
}

impl core::fmt::Debug for FrameTimer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FrameTimer")
            .field("period_ns", &self.period_ns)
            .field("fresh", &self.fresh)
            .field("dropped", &self.dropped)
            .field("in_flight", &self.map_in_flight)
            // Query set and buffers are opaque; the counters are the state
            // worth printing, and `armed`/`awaiting_submit` are a strict
            // sub-phase of `in_flight` for a reader's purposes.
            .finish_non_exhaustive()
    }
}

impl FrameTimer {
    /// Build a timer on `device`, or `None` when the device cannot time
    /// the GPU.
    ///
    /// `None` is returned rather than an error because a host without
    /// timestamp support is not a broken host — it is one where this
    /// measurement does not exist. Callers keep rendering and report no
    /// GPU time, and the absence of a timer is the one source for that
    /// fact — [`crate::GpuContext`] deliberately does not keep a second
    /// copy of it that could disagree.
    #[must_use]
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        let needed =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        if !device.features().contains(needed) {
            return None;
        }
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("pinion-gpu frame timer"),
            ty: wgpu::QueryType::Timestamp,
            count: QUERY_COUNT,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pinion-gpu frame timer resolve"),
            size: RESOLVE_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pinion-gpu frame timer staging"),
            size: RESOLVE_BYTES,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Self {
            query_set,
            resolve,
            staging,
            period_ns: queue.get_timestamp_period(),
            signal: Arc::new(AtomicU8::new(MAP_WAITING)),
            armed: false,
            awaiting_submit: false,
            map_in_flight: false,
            fresh: None,
            dropped: 0,
        })
    }

    /// Record the opening timestamp into `encoder`, which the caller must
    /// submit **before** any of the frame's rendering work.
    ///
    /// Returns whether this frame is being timed: `false` when a previous
    /// frame's result is still in flight, in which case the caller should
    /// not call [`Self::end`] for this frame either. Skipping is safe — it
    /// costs a sample, not correctness — and it is what keeps a slow GPU
    /// from making the timer report spans that start in one frame and end
    /// in another.
    pub fn begin(&mut self, encoder: &mut wgpu::CommandEncoder) -> bool {
        if self.map_in_flight || self.awaiting_submit {
            return false;
        }
        encoder.write_timestamp(&self.query_set, 0);
        self.armed = true;
        true
    }

    /// Record the closing timestamp into `encoder` — the frame's blit
    /// encoder — and resolve the pair into the staging buffer.
    ///
    /// A no-op when [`Self::begin`] did not arm this frame, so a caller
    /// that returns early between the two (a lost swapchain, a zero-sized
    /// surface) leaves no half-written measurement behind.
    pub fn end(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if !self.armed {
            return;
        }
        encoder.write_timestamp(&self.query_set, 1);
        encoder.resolve_query_set(&self.query_set, 0..QUERY_COUNT, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.staging, 0, RESOLVE_BYTES);
        self.armed = false;
        self.awaiting_submit = true;
    }

    /// Ask for the staging buffer once the encoder [`Self::end`] wrote has
    /// been submitted.
    ///
    /// Separate from `end` because `map_async` on a buffer whose producing
    /// commands have not been submitted is a request that can never be
    /// satisfied by a poll — the work it waits on does not exist yet.
    pub fn after_submit(&mut self) {
        if !self.awaiting_submit {
            return;
        }
        self.awaiting_submit = false;
        self.signal.store(MAP_WAITING, Ordering::SeqCst);
        let signal = Arc::clone(&self.signal);
        self.staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                signal.store(
                    if result.is_ok() {
                        MAP_READY
                    } else {
                        MAP_FAILED
                    },
                    Ordering::SeqCst,
                );
            });
        self.map_in_flight = true;
    }

    /// Harvest a finished measurement if one is ready. Non-blocking.
    ///
    /// Call once per frame, before [`Self::begin`]. The `poll` is what
    /// gives `wgpu` the chance to run map callbacks on a native backend;
    /// without it the callback fires only when some other code happens to
    /// poll, and the timer would look permanently stalled.
    pub fn collect(&mut self, device: &wgpu::Device) {
        if !self.map_in_flight {
            return;
        }
        // `Poll` — never `Wait`. `Wait` here would block the CPU until the
        // GPU drained, which is the stall this whole design exists to
        // avoid; the result simply arrives on a later frame instead.
        let _ = device.poll(wgpu::PollType::Poll);
        match self.signal.load(Ordering::SeqCst) {
            MAP_READY => {
                let mut ticks = [0_u64; 2];
                {
                    let view = self.staging.slice(..).get_mapped_range();
                    for (i, slot) in ticks.iter_mut().enumerate() {
                        let mut raw = [0_u8; 8];
                        raw.copy_from_slice(&view[i * 8..i * 8 + 8]);
                        *slot = u64::from_le_bytes(raw);
                    }
                }
                self.staging.unmap();
                self.map_in_flight = false;
                match self.span_us(ticks[0], ticks[1]) {
                    Some(us) => self.fresh = Some(us),
                    None => self.dropped = self.dropped.saturating_add(1),
                }
            }
            MAP_FAILED => {
                // Deliberately not unmapped: the map never succeeded, so
                // there is nothing mapped to release, and `unmap` on an
                // unmapped buffer raises a wgpu validation error that the
                // uncaptured-error handler would surface as noise about a
                // second, non-existent fault. The sample is counted as
                // dropped so a host where this keeps happening is visible
                // in the numbers rather than silently timing nothing.
                self.map_in_flight = false;
                self.dropped = self.dropped.saturating_add(1);
            }
            _ => {}
        }
    }

    /// Take the measurement harvested since the last call, as a
    /// [`GpuFrameClock`].
    ///
    /// **Consuming, and `&mut self` says so.** Each measurement is
    /// reported exactly once, so a caller stamping one frame sample per
    /// call records distinct measurements rather than the same one
    /// repeated. Without that, `N` frames sharing one reading would look
    /// like `N` samples: a mean over them would be a mean of one number,
    /// and a count of "samples carrying a timing" would be counting
    /// frames. Both would still look perfectly healthy.
    ///
    /// [`GpuFrameClock::Pending`] therefore means *nothing new since you
    /// last asked* — which covers a timer that has never produced one and
    /// a frame that arrived faster than the GPU could report. Neither is
    /// an error, and a caller that needs the persisting level rather than
    /// the persisting level reads the ring fold a caller already keeps
    /// (`mean_gpu_us` / `max_gpu_us`), not a field here.
    ///
    /// A live timer is never [`GpuFrameClock::Unsupported`] — the type
    /// cannot be constructed on a device without timestamp queries — so
    /// that arm belongs to the *absence* of a timer, and it is the caller
    /// holding the `Option<FrameTimer>` that maps it.
    pub fn clock(&mut self) -> GpuFrameClock {
        self.fresh
            .take()
            .map_or(GpuFrameClock::Pending, GpuFrameClock::Measured)
    }

    /// Measurements that were taken but discarded — a failed map, or a
    /// tick pair the device reported out of order.
    ///
    /// Published rather than swallowed because a timer that discards
    /// everything and one on a window that simply has not sampled yet are
    /// otherwise the same two values forever ([`GpuFrameClock::Pending`]
    /// with nothing on the wire), and the documented advice for that pair
    /// is "read again in a frame".
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Convert a resolved tick pair to microseconds, or `None` when the
    /// pair is not physically meaningful.
    ///
    /// Both timestamps are written into one queue, whose execution `wgpu`
    /// orders, so `end < start` cannot describe a real frame — it is a
    /// driver artifact (an unsynchronised timestamp domain, a reset
    /// counter). Reporting it as a huge or wrapped duration would inject
    /// fictional jank into exactly the statistic someone is using to hunt
    /// jank, so the sample is dropped instead.
    fn span_us(&self, start: u64, end: u64) -> Option<u64> {
        span_us_from(start, end, self.period_ns)
    }
}

/// Convert a resolved tick pair to microseconds, or `None` when the pair is
/// not physically meaningful.
///
/// A free function so it can be tested without a GPU. The device-dependent
/// part is one `f32`, and the branch worth testing is the one that DISCARDS
/// a sample — which is otherwise reachable only by finding a driver that
/// misbehaves.
fn span_us_from(start: u64, end: u64, period_ns: f32) -> Option<u64> {
    let ticks = end.checked_sub(start)?;
    // Precision: `f64` holds a u64 exactly below 2^53 ticks, which at
    // sub-nanosecond periods is over a month of continuous GPU time in a
    // single frame. Truncation to `u64` microseconds is the intended
    // rounding — the value is integral microseconds.
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "see the precision note above; ticks < 2^53 and the result is integral us"
    )]
    let us = (f64::from(period_ns) * ticks as f64 / 1000.0) as u64;
    Some(us)
}

#[cfg(test)]
mod tests {
    use super::{GpuFrameClock, span_us_from};

    /// A common `timestampPeriod`: 1ns per tick.
    const NS: f32 = 1.0;

    #[test]
    fn r1537_out_of_order_ticks_are_discarded_not_reported() {
        // Both timestamps go into ONE queue, whose execution wgpu orders, so
        // `end < start` cannot describe a real frame — it is a driver
        // artifact (an unsynchronised timestamp domain, a reset counter).
        //
        // The failure this guards is not "a wrong number": a wrapping
        // subtraction would report ~18 million seconds of GPU time, which is
        // fictional jank injected into the exact statistic someone is using
        // to hunt jank. Dropping the sample is the only honest answer, and
        // `gpu_dropped_total` is where it becomes visible.
        assert_eq!(span_us_from(10_000, 9_000, NS), None);
        assert_eq!(span_us_from(1, 0, NS), None);
        assert_eq!(span_us_from(u64::MAX, 0, NS), None);
    }

    #[test]
    fn r1537_a_span_converts_ticks_to_integral_microseconds() {
        assert_eq!(span_us_from(0, 1_000, NS), Some(1));
        assert_eq!(span_us_from(5_000, 1_005_000, NS), Some(1_000));
        // Equal timestamps are a real frame below the timer's resolution —
        // `Some(0)`, which is a measurement, not the absence of one.
        assert_eq!(span_us_from(42, 42, NS), Some(0));
        // Sub-microsecond spans truncate toward zero rather than vanishing
        // into `None`: the frame WAS measured.
        assert_eq!(span_us_from(0, 999, NS), Some(0));
    }

    #[test]
    fn r1537_the_device_period_scales_the_span() {
        // A device reporting 38.4ns/tick (a common AMD value) must not be
        // read as if it reported 1ns/tick — the tick count alone is
        // meaningless, which is why the period is captured from the queue
        // rather than assumed.
        assert_eq!(span_us_from(0, 1_000, 38.4), Some(38));
        assert_ne!(span_us_from(0, 1_000, 38.4), span_us_from(0, 1_000, NS));
    }

    #[test]
    fn r1537_absent_and_pending_are_not_the_same_answer() {
        // Both yield `None` from `measured()`, which is why `measured()`
        // alone must never be the whole story: one is a permanent property
        // of the machine and the other resolves next frame.
        assert_eq!(GpuFrameClock::Unsupported.measured(), None);
        assert_eq!(GpuFrameClock::Pending.measured(), None);
        assert_ne!(GpuFrameClock::Unsupported, GpuFrameClock::Pending);
        assert!(!GpuFrameClock::Unsupported.is_supported());
        assert!(
            GpuFrameClock::Pending.is_supported(),
            "pending means the timer is running and has nothing new to say — \
             a caller that treats it as unsupported stops waiting for a \
             measurement that is on its way",
        );
    }

    #[test]
    fn r1537_a_measured_zero_is_a_measurement() {
        // The distinction the whole `Option`/enum shape exists for: a GPU
        // frame below the timer's resolution is measured, and must not be
        // confused with a host that cannot measure.
        let fast = GpuFrameClock::Measured(0);
        assert_eq!(fast.measured(), Some(0));
        assert!(fast.is_supported());
        assert_ne!(fast, GpuFrameClock::Unsupported);
        assert_ne!(fast, GpuFrameClock::Pending);
    }
}

//! ★★★★★ R2010 §5.35 §5.15 §2 #2 — **drive an assembled application's pointer
//! the way its window does, and ask its surfaces questions in the frame they
//! were placed in.**
//!
//! # The hole this closes
//!
//! §2 #7 makes a pinion screen ONE [`External`], and a screen roster mounts
//! screens inside a host that is itself one. So an assembled application is a
//! *tree of surfaces*, and delivering a press into it is not one call — it is
//! five things that must all be true at once:
//!
//! 1. the model the router dispatches into holds **every** surface, not just
//!    the host's ([`CoreShell::state_scene`]);
//! 2. each surface's size has been **announced** from the rectangle the paint
//!    put it in ([`announce_external_sizes`]), because a pointer travels as a
//!    FRACTION and that store is the basis it is multiplied back out by;
//! 3. the router holds the paint, so a window point resolves to a surface at
//!    all ([`InputRouter::update_paint_scene`]);
//! 4. every surface is **granted the extent it was placed in** for the duration
//!    of the call, because
//!    [`layout_size`](pinion_core::external::layout_size) prefers a live
//!    `Owner` scope's viewport over the recorded size — so a guest asked
//!    anything inside a scope answers about the HOST's window; and
//! 5. the cursor is moved before the press, because a press carries no position
//!    of its own.
//!
//! Nothing published those five as one thing, so every caller that wanted to
//! drive an assembled application wrote them out — and getting any one of them
//! wrong makes a press vanish with no error anywhere.
//!
//! # The measurement this comes from
//!
//! The analysis tool's own walk could not press a control on a mounted screen,
//! and the cause was three of the five in a row, each hiding the next:
//!
//! | step | what it did instead | what a press did |
//! |---|---|---|
//! | 1 | built the model as *the host's `External`, alone* | resolved to the guest's tag and reached no surface |
//! | 2 | never announced a size | read the `(1, 1)` fallback, flooring every fraction to zero |
//! | 4 | ran inside an owner scope with no grant | the guest resolved a cursor against a window **52 pixels wider** than the one it had been painted into, and missed |
//!
//! ★★★★★ And step 4 is **not only the delivery's**. Measured at R2010 over the
//! six screens that tool mounts: asking each guest what its own painted marks
//! address — [`External::target_of_tag`] against [`External::target_at`], the
//! pair a pointer-target census is built on — the two answers disagreed on
//! **76 of 504** marks when the question was asked outside the grants and on
//! **2** when it was asked inside them, both of those being the
//! group-with-a-grip case that census already has a word for. So the grant
//! belongs to the QUESTION as much as to the event, which is why
//! [`ask`](DrivenPointer::ask) exists beside [`press`](DrivenPointer::press).
//!
//! # Where the floor stands
//!
//! A mature toolkit publishes synthetic pointer delivery into an assembled
//! application (its test module's press / click helpers, aimed at a widget),
//! and that is the capability this meets. What it does not have to solve is
//! step 4: its widgets are one tree in one window, so there is no second frame
//! for an answer to come back in. A composed-surface model has one at every
//! mount, and [`DrivenPointer::ask`] is the half of this that has no
//! counterpart there.
//!
//! # What this is NOT
//!
//! Not a substitute for the window. It performs the pointer half of a frame —
//! it does not paint, settle, or pump a real event loop, and it takes the paint
//! it is given. A caller that wants the next frame paints it and opens a new
//! session over it, which is what a window does too.
//!
//! [`External`]: pinion_core::external::External
//! [`External::target_at`]: pinion_core::external::External::target_at
//! [`External::target_of_tag`]: pinion_core::external::External::target_of_tag
//! [`CoreShell::state_scene`]: crate::core_shell::CoreShell::state_scene
//! [`InputRouter::update_paint_scene`]: crate::input::InputRouter::update_paint_scene

use crate::core_shell::CoreShell;
use crate::input::{ExternalSizes, InputRouter, PointerId, announce_external_sizes};
use pinion_core::external::{External, with_surface_extent};
use pinion_core::reactive::Owner;
use pinion_core::{Scene, WidgetCore};
use std::collections::BTreeMap;

/// ★★★★★ R2010 — a pointer driven into an assembled application, through the
/// same router the running window drives.
///
/// Open one over a painted frame with [`DrivenPointer::over`], move it with
/// [`cursor`](Self::cursor), and [`press`](Self::press) /
/// [`release`](Self::release). Every call runs with every surface granted the
/// extent the paint placed it in, so a mounted screen resolves the pointer
/// against its own window rather than its host's.
pub struct DrivenPointer {
    router: InputRouter,
    model: Scene,
    /// Surface tag -> the extent the paint placed it in. Ordered so the grants
    /// nest in a stated order rather than a hash's.
    placed: BTreeMap<String, (u32, u32)>,
}

impl DrivenPointer {
    /// Open a session over `paint`, with **the whole surface set of `V`**
    /// behind it.
    ///
    /// The model is [`CoreShell::state_scene`](crate::core_shell::CoreShell::state_scene)
    /// — the same derivation the running application boots from — so a mounted
    /// screen's surface is present for the same reason it is present in
    /// production, rather than because a caller remembered to add it.
    ///
    /// # Owner
    ///
    /// Both surface factories run inside `owner`, so they resolve the same
    /// reactive state the view function does. Pass the scope the application's
    /// view runs in; a different one builds a second copy of every screen's
    /// state, which this cannot detect.
    #[must_use]
    pub fn over<V: WidgetCore>(owner: &Owner, paint: Scene) -> Self {
        let mut model = CoreShell::<V>::state_scene(owner);
        let mut surfaces: Vec<String> = Vec::new();
        model.for_each_node(&mut |visit| {
            if matches!(visit.node, Scene::External(_)) {
                if let Some(tag) = visit.node.tag() {
                    surfaces.push(tag.to_owned());
                }
            }
        });
        let mut placed: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        paint.for_each_node(&mut |visit| {
            let (Some(tag), Some(rect)) = (visit.node.tag(), visit.absolute_rect()) else {
                return;
            };
            if rect.w > 0 && rect.h > 0 && surfaces.iter().any(|known| known == tag) {
                placed.insert(tag.to_owned(), (rect.w, rect.h));
            }
        });
        // Step 2, inside the grants of step 4: `on_resize` may re-run a
        // screen's own layout, and a screen laid out against its host's
        // viewport is the defect this whole module is about.
        let mut known = ExternalSizes::default();
        granting(&placed, || {
            announce_external_sizes(&paint, &mut model, &mut known);
        });
        let mut router = InputRouter::new();
        router.update_paint_scene(paint, &mut model);
        Self {
            router,
            model,
            placed,
        }
    }

    /// The surface a press would be delivered to — what the router is hovering.
    #[must_use]
    pub fn hovering(&self) -> Option<&str> {
        self.router.hover_target(PointerId::MOUSE)
    }

    /// The extent `surface` was placed in by the paint this session was opened
    /// over, or `None` for a tag that names no painted surface.
    #[must_use]
    pub fn placed(&self, surface: &str) -> Option<(u32, u32)> {
        self.placed.get(surface).copied()
    }

    /// Every surface this session drives, in tag order.
    ///
    /// Published because a session that silently holds fewer surfaces than the
    /// application does is exactly the first of the five failures above, and a
    /// caller cannot notice it from a press that does nothing.
    pub fn surfaces(&self) -> impl Iterator<Item = &str> {
        self.placed.keys().map(String::as_str)
    }

    /// The state scene the router dispatches into.
    #[must_use]
    pub fn model(&self) -> &Scene {
        &self.model
    }

    /// Move the pointer to a window point.
    pub fn cursor(&mut self, at: (u32, u32)) {
        let Self {
            router,
            model,
            placed,
        } = self;
        granting(placed, || {
            router.cursor_moved(PointerId::MOUSE, f64::from(at.0), f64::from(at.1), model);
        });
    }

    /// Press the primary button where the pointer is.
    pub fn press(&mut self) {
        let Self {
            router,
            model,
            placed,
        } = self;
        granting(placed, || {
            router.pointer_down(PointerId::MOUSE, model);
        });
    }

    /// Release the primary button where the pointer is.
    pub fn release(&mut self) {
        let Self {
            router,
            model,
            placed,
        } = self;
        granting(placed, || {
            router.pointer_up(PointerId::MOUSE, model);
        });
    }

    /// Ask `surface` a question **in the frame it was placed in**, or `None`
    /// when no surface carries that tag.
    ///
    /// The grants are the same ones a delivery runs under, for the reason the
    /// module header measures: a guest asked anything from inside a live
    /// `Owner` scope answers about its host's window unless its own placement
    /// is stated. That makes an ungranted question a second, quieter version of
    /// the defect this type exists for — it does not vanish, it answers wrongly.
    pub fn ask<R>(&self, surface: &str, question: impl FnOnce(&dyn External) -> R) -> Option<R> {
        let external = self.model.find_external_with_tag(surface)?;
        Some(granting(&self.placed, || question(&*external.handle)))
    }
}

/// Run `body` with every surface granted the extent it was placed in.
///
/// Nested rather than sequential because
/// [`with_surface_extent`](pinion_core::external::with_surface_extent) states a
/// grant for the duration of a call, and every surface must be stated at once:
/// a host builds its scene while its guests are placed, and a guest asked
/// during that has to be able to read its own rectangle.
fn granting<R>(placed: &BTreeMap<String, (u32, u32)>, body: impl FnOnce() -> R) -> R {
    fn nest<R>(
        mut rest: std::collections::btree_map::Iter<'_, String, (u32, u32)>,
        body: impl FnOnce() -> R,
    ) -> R {
        match rest.next() {
            None => body(),
            Some((tag, extent)) => with_surface_extent(tag, *extent, || nest(rest, body)),
        }
    }
    nest(placed.iter(), body)
}

#[cfg(test)]
mod tests {
    use super::DrivenPointer;
    use pinion_core::external::{
        Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
        IntrospectSchema, IntrospectValue, InvokeError, PointerTarget, ReadRefusal, RepaintOwner,
        SchemaField, ThreadOwnership,
    };
    use pinion_core::input::PointerReading;
    use pinion_core::reactive::Owner;
    use pinion_core::scene::{ContainerNode, ExternalNode, Rect};
    use pinion_core::widget_core::ExtraExternal;
    use pinion_core::{Frame, Scene, WidgetCore};
    use std::cell::Cell;

    const HOST: &str = "host";
    const GUEST: &str = "guest";
    /// The rectangle the host places the guest in — deliberately smaller than
    /// the viewport below, because equal sizes are what make the defect this
    /// module is about invisible.
    const GUEST_RECT: (u32, u32, u32, u32) = (40, 30, 300, 200);
    const VIEWPORT: (u32, u32) = (800, 600);

    thread_local! {
        /// The pixel the guest last resolved a pointer to.
        static GUEST_AT: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
        /// How many presses the guest has been sent.
        static GUEST_PRESSES: Cell<u32> = const { Cell::new(0) };
        /// The window the guest believed it was in when it last answered.
        static GUEST_WINDOW: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
    }

    /// A guest that hit-tests itself, exactly as a mounted screen does: it is
    /// handed a FRACTION and multiplies it back out through the framework's own
    /// expression, which is what makes it answer about whichever window it is
    /// told it is in.
    #[derive(Debug)]
    struct Guest;

    impl External for Guest {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
        }

        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }

        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }

        /// A screen that hit-tests itself has to be told where the pointer is
        /// before it is pressed, which is the opt-in every mounted screen in
        /// this tree declares.
        fn wants_hover_move(&self) -> bool {
            true
        }

        fn pointer_move(&mut self, at: PointerReading) {
            GUEST_WINDOW.with(|w| w.set(window_of(GUEST)));
            GUEST_AT.with(|cell| cell.set(pinion_core::external::layout_point(GUEST, at.at)));
        }

        fn target_at(&self, x: u32, y: u32) -> PointerTarget {
            let (w, h) = window_of(GUEST);
            // The right half of whatever window it believes it is in.
            if x >= w / 2 && y < h {
                PointerTarget::Word("right".into())
            } else {
                PointerTarget::Nothing
            }
        }

        fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
            Some(self)
        }
    }

    impl ExternalIntrospect for Guest {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(const { &[SchemaField::action("send", "text")] })
        }

        fn query(&self, _path: &str) -> Result<IntrospectValue, ReadRefusal> {
            Err(ReadRefusal::UnknownPath)
        }

        fn intervene(
            &mut self,
            _path: &str,
            _value: IntrospectValue,
        ) -> Result<(), InterveneError> {
            Err(InterveneError::UnknownPath)
        }

        fn invoke(
            &mut self,
            path: &str,
            _args: IntrospectValue,
        ) -> Result<IntrospectValue, InvokeError> {
            if path == "send" {
                GUEST_PRESSES.with(|c| c.set(c.get() + 1));
                return Ok(IntrospectValue::Bool(true));
            }
            Err(InvokeError::UnknownPath)
        }
    }

    /// What [`pinion_core::external::layout_size`] answers for `tag` — the
    /// three-source read the module header names, asked the way a screen asks
    /// it of itself.
    fn window_of(tag: &str) -> (u32, u32) {
        pinion_core::external::layout_size(tag, (1, 1), (1, 1))
    }

    #[derive(Debug)]
    struct HostSurface;

    impl External for HostSurface {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
        }

        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }

        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
    }

    struct HostView;

    impl WidgetCore for HostView {
        type State = ();
        type Event = ();

        fn create_external() -> Box<dyn External> {
            Box::new(HostSurface)
        }

        fn tag() -> &'static str {
            HOST
        }

        fn create_extra_externals() -> Vec<ExtraExternal> {
            vec![ExtraExternal::new(GUEST, Box::new(Guest))]
        }

        fn read_state(_scene: &Scene) -> Self::State {}

        fn view(_state: Self::State, _frame: &Frame) -> Scene {
            paint()
        }

        fn event_name(_event: Self::Event) -> &'static str {
            "__internal__"
        }

        fn title() -> &'static str {
            "Host"
        }
    }

    /// The host's frame: itself filling the viewport, with the guest placed in
    /// a rectangle of its own.
    fn paint() -> Scene {
        let mut guest = ExternalNode::new(Box::new(Guest)).with_tag(GUEST);
        guest.rect = Rect::new(GUEST_RECT.0, GUEST_RECT.1, GUEST_RECT.2, GUEST_RECT.3);
        let mut root = ContainerNode::new(vec![Scene::External(guest)]).with_tag(HOST);
        root.rect = Rect::new(0, 0, VIEWPORT.0, VIEWPORT.1);
        Scene::Container(root)
    }

    fn in_a_window<R>(body: impl FnOnce(&Owner) -> R) -> R {
        let owner = Owner::new();
        owner.run(|| {
            pinion_core::reactive::VIEWPORT_SIZE
                .resolve(&owner)
                .set(VIEWPORT);
            body(&owner)
        })
    }

    #[test]
    fn r2010_a_session_holds_every_surface_the_application_composes() {
        in_a_window(|owner| {
            let hand = DrivenPointer::over::<HostView>(owner, paint());
            let surfaces: Vec<&str> = hand.surfaces().collect();
            assert!(
                surfaces.contains(&GUEST),
                "the guest's surface is in the session, which is what a press \
                 aimed at it has to reach: {surfaces:?}",
            );
            assert_eq!(
                hand.placed(GUEST),
                Some((GUEST_RECT.2, GUEST_RECT.3)),
                "and it is held at the extent the PAINT placed it in, not at \
                 the window's",
            );
        });
    }

    #[test]
    fn r2010_a_press_over_a_guest_is_delivered_to_that_guest() {
        in_a_window(|owner| {
            GUEST_PRESSES.with(|c| c.set(0));
            let mut hand = DrivenPointer::over::<HostView>(owner, paint());
            hand.cursor((GUEST_RECT.0 + 10, GUEST_RECT.1 + 10));
            assert_eq!(
                hand.hovering(),
                Some(GUEST),
                "a cursor inside the guest's rectangle hovers the guest, so the \
                 press that follows is addressed to it",
            );
            hand.press();
            hand.release();
            assert!(
                GUEST_PRESSES.with(Cell::get) > 0,
                "the guest was sent the press",
            );
        });
    }

    #[test]
    fn r2010_a_guest_resolves_the_pointer_against_its_own_window() {
        in_a_window(|owner| {
            let mut hand = DrivenPointer::over::<HostView>(owner, paint());
            let inside = (37, 21);
            hand.cursor((GUEST_RECT.0 + inside.0, GUEST_RECT.1 + inside.1));
            assert_eq!(
                GUEST_WINDOW.with(Cell::get),
                (GUEST_RECT.2, GUEST_RECT.3),
                "the guest was told which window it is in; without the grant it \
                 reads the enclosing scope's viewport, which is the host's",
            );
            assert_eq!(
                GUEST_AT.with(Cell::get),
                inside,
                "so the pixel it resolves is the one the pointer is over",
            );
        });
    }

    /// ★★★★★ The counterfactual for the grant, run rather than described: the
    /// SAME question, asked outside the session, answers about the host's
    /// window — which is what made 76 of one application's marks disagree with
    /// themselves.
    #[test]
    fn r2010_a_question_asked_outside_the_grant_answers_about_the_wrong_window() {
        in_a_window(|owner| {
            let hand = DrivenPointer::over::<HostView>(owner, paint());
            // A point in the right half of the GUEST's own window, and in the
            // left half of the host's viewport.
            let probe = (GUEST_RECT.2 / 2 + 5, 10);
            let granted = hand
                .ask(GUEST, |guest| guest.target_at(probe.0, probe.1))
                .expect("the guest is a surface of this session");
            assert_eq!(
                granted.word(),
                Some("right"),
                "asked in the frame it was placed in, the guest answers about \
                 its own rectangle",
            );
            let ungranted = Guest.target_at(probe.0, probe.1);
            assert_eq!(
                ungranted,
                PointerTarget::Nothing,
                "and the same question outside the grant answers about the \
                 HOST's window — the quiet half of this defect, which returns a \
                 wrong answer rather than none",
            );
        });
    }
}

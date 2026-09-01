//! ★★★★★ R1948 — what this section claims about itself, asserted rather than
//! written down.

use super::{Hit, spec, use_view_state};

/// The state as the section opens. The cache is shared across a thread, so a
/// test that changed the selection would decide what the next one sees.
fn fresh() -> std::rc::Rc<super::ViewState> {
    let state = use_view_state();
    state.selected.set(spec::OPENS_ON.to_owned());
    state.chip.set(0);
    state.crossing.set(None);
    state
}

/// ★★★★★ R1948 — **every session names a peer the topology section plots.**
///
/// The round's central claim, and the reason these are two views of one capture
/// rather than two screens sharing a rail: the reference's detail pane crosses
/// to the graph with this peer selected, and a session naming a peer the graph
/// does not have would send a reader somewhere that is not there.
///
/// ⚠ Asserted against `hello_topology_view::peers()` — that section's own
/// published population — rather than against a copy. Two tables kept in step
/// by hand are two tables that drift.
#[test]
fn r1948_every_session_names_a_peer_the_topology_plots() {
    let peers = hello_topology_view::peers();
    assert!(
        !peers.is_empty(),
        "the topology section plots nothing, so this comparison judges an empty set",
    );
    for session in spec::SESSIONS {
        assert!(
            peers.contains(&session.peer),
            "{} reaches {}, which the topology section does not plot",
            session.id,
            session.peer,
        );
    }
    // ★★★★★ AND THE OTHER DIRECTION IS NOT THE MIRROR, which this test found by
    // being wrong about it. The first draft asserted every plotted node has a
    // session and failed on `R-01`: the ROUTER is not the far end of a session,
    // it is what the sessions pass THROUGH. So the honest statements are that
    // each session reaches a distinct peer, and that the graph holds strictly
    // more nodes than there are sessions — the difference being the routers.
    let mut reached: Vec<&str> = spec::SESSIONS.iter().map(|s| s.peer).collect();
    reached.sort_unstable();
    let distinct = reached.len();
    reached.dedup();
    assert_eq!(
        reached.len(),
        distinct,
        "two sessions reach one peer, so the peer column is not a key",
    );
    assert!(
        peers.len() > spec::SESSIONS.len(),
        "the graph holds {} node(s) and the capture has {} session(s); the difference \
         is what routes rather than terminates, and a graph with none of those is not \
         the topology this capture describes",
        peers.len(),
        spec::SESSIONS.len(),
    );
}

/// ★★★★★ R1948 — **every standing is a word the graded vocabulary holds.**
#[test]
fn r1948_every_standing_is_graded_by_the_scale_this_application_uses() {
    for session in spec::SESSIONS {
        assert!(
            spec::SEVERITY.rank(session.standing.severity()).is_some(),
            "{} is {:?}, graded {:?}, which the vocabulary does not hold",
            session.id,
            session.standing.label(),
            session.standing.severity(),
        );
    }
    assert!(
        spec::SESSIONS.len() > 1,
        "a capture of one session does not exercise a list",
    );
}

/// ★★★★★ R1948 — **every standing the vocabulary declares is reached.**
///
/// The half that catches the likelier error: a vocabulary can be correct and
/// unused. If `Closed` were declared and no session were closed, the list would
/// never draw the failed state and no reader would recognise it.
#[test]
fn r1948_every_standing_is_drawn_by_some_session() {
    for standing in [
        spec::Standing::Established,
        spec::Standing::Reconnecting,
        spec::Standing::Closed,
    ] {
        assert!(
            spec::SESSIONS.iter().any(|s| s.standing == standing),
            "no session is {:?}, so the list never draws that state",
            standing.label(),
        );
    }
}

/// ★★★★★ R1948 — **the header's count is the population's, in both halves.**
///
/// The reference writes `5 active / 1 closed` into its markup. This build
/// derives both, and the assertion is that they PARTITION the list — which is
/// what a count beside a list has to be true of, and what a hand-written pair
/// stops being the first time the capture changes.
#[test]
fn r1948_the_headers_count_partitions_the_sessions() {
    let (active, closed) = spec::tally();
    assert_eq!(
        active + closed,
        spec::SESSIONS.len(),
        "the two halves of the count do not add up to the list",
    );
    assert!(active > 0, "no session is active");
    assert!(
        closed > 0,
        "no session is closed, so `closed` is never shown"
    );
    // And `active` means what the state says, not what a number remembers.
    assert_eq!(
        active,
        spec::SESSIONS
            .iter()
            .filter(|s| s.standing.is_active())
            .count(),
        "the count and the predicate disagree",
    );
}

/// ★★★★★ R1948 — **every column answers for every session.**
///
/// `cell` matches on the column key and panics on an unknown one, so this walks
/// the whole product: a column the table gains has to be answered or this test
/// is the thing that reports it, rather than a blank appearing on the screen.
#[test]
fn r1948_every_column_answers_for_every_session() {
    assert_eq!(
        spec::COLUMNS.len(),
        8,
        "the reference's grid has eight tracks"
    );
    for session in spec::SESSIONS {
        for column in spec::COLUMNS {
            let cell = session.cell(column.key);
            assert!(
                !cell.is_empty(),
                "{}'s {} cell is empty — a blank is indistinguishable from a value \
                 that failed to draw",
                session.id,
                column.key,
            );
        }
    }
    // ★ Exactly one column takes the slack. Two would make the grid's width
    // ambiguous, none would leave it unable to fill the pane.
    assert_eq!(
        spec::COLUMNS.iter().filter(|c| c.stretches).count(),
        1,
        "the reference gives exactly one track the slack",
    );
}

/// ★★★★★ R1948 — **the handshake is derived from the standing, and says so.**
///
/// Every session takes the same four steps; the fifth is what its state
/// determines. A timeline stored per row could claim a handshake the row's own
/// status contradicts, which is the defect this shape makes unrepresentable.
#[test]
fn r1948_the_handshake_follows_the_standing() {
    for session in spec::SESSIONS {
        let steps = session.timeline();
        assert!(
            steps.len() >= 5,
            "{}'s handshake has {} steps",
            session.id,
            steps.len()
        );
        for step in &steps {
            assert!(
                spec::SEVERITY.rank(step.severity).is_some(),
                "{} has a step graded {:?}, which the vocabulary does not hold",
                session.id,
                step.severity,
            );
        }
        let last = steps.last().expect("the length was just asserted");
        match session.standing {
            spec::Standing::Established => assert_eq!(last.severity, "info"),
            spec::Standing::Reconnecting => assert_eq!(last.severity, "warn"),
            spec::Standing::Closed => assert_eq!(last.severity, "error"),
        }
    }
}

/// ★★★★★ R1948 — **no session's handshake is longer than the space the layout
/// reserves for it.**
///
/// `spec::MAX_HANDSHAKE_STEPS` is a constant because the geometry that reads it
/// is a `const fn`, and a constant nothing checks is the shape this tree keeps
/// finding wrong. This is the check: a standing that grew a step would push the
/// channel list past the floor the window declares, and it would do it
/// silently.
#[test]
fn r1948_the_handshake_fits_the_space_reserved_for_it() {
    let longest = spec::SESSIONS
        .iter()
        .map(|s| s.timeline().len())
        .max()
        .unwrap_or(0);
    assert!(
        u32::try_from(longest).unwrap_or(u32::MAX) <= spec::MAX_HANDSHAKE_STEPS,
        "the longest handshake has {longest} steps and the layout reserves {}",
        spec::MAX_HANDSHAKE_STEPS,
    );
    // ★ And the reservation is not wildly loose: a number far above the truth
    // pushes the floor out for nothing, which costs a reader window height.
    assert!(
        u32::try_from(longest).unwrap_or(0) + 1 >= spec::MAX_HANDSHAKE_STEPS,
        "the layout reserves {} steps for a handshake that is at most {longest}",
        spec::MAX_HANDSHAKE_STEPS,
    );
}

/// ★★★★★ R1948 — **a closed session states its sequence numbers are gone.**
///
/// In words rather than as a blank: a reader cannot tell an empty cell from a
/// value that failed to draw, and the reference makes the same choice.
#[test]
fn r1948_a_closed_session_says_its_channels_are_over() {
    let closed = spec::SESSIONS
        .iter()
        .find(|s| s.standing == spec::Standing::Closed)
        .expect("the population declares one, which the sibling test asserts");
    for channel in spec::CHANNELS {
        assert_eq!(
            closed.sequence(channel),
            "-",
            "{}'s {} still reports a sequence",
            closed.id,
            channel.name,
        );
    }
    let live = spec::SESSIONS
        .iter()
        .find(|s| s.standing != spec::Standing::Closed)
        .expect("a capture of only closed sessions would fail the count test");
    for channel in spec::CHANNELS {
        assert_ne!(
            live.sequence(channel),
            "-",
            "{}'s {} reports nothing while the session is up",
            live.id,
            channel.name,
        );
    }
}

/// ★★★★★ R1948 — **a press by point and a press by tag are one behaviour.**
#[test]
fn r1948_a_hit_resolves_the_same_from_a_point_and_from_a_tag() {
    let state = fresh();
    for (visual, session) in state.kept().iter().enumerate() {
        let row = super::row_rect(visual);
        let by_point = Hit::at(&state, row.x + row.w / 2, row.y + row.h / 2);
        let by_tag = Hit::of_tag(&format!("sv.row.{}", session.id));
        assert_eq!(
            by_point, by_tag,
            "{} resolves differently by point and by tag",
            session.id
        );
        assert!(by_tag.word().is_some(), "{} answers no word", session.id);
    }
}

/// ★★★★★ R1948 — **a chip narrows the list, and the detail follows it.**
///
/// The second half is what a naive filter gets wrong: if the picked session is
/// filtered away, a detail pane describing it is a panel about something the
/// reader cannot see. Asserted through the chip that keeps exactly one state.
#[test]
fn r1948_choosing_a_chip_narrows_the_list_and_carries_the_detail() {
    let state = fresh();
    let all = state.kept().len();
    assert_eq!(
        all,
        spec::SESSIONS.len(),
        "the section opens showing every session"
    );
    for (n, chip) in spec::CHIPS.iter().enumerate() {
        let Some(standing) = chip.keeps else {
            continue;
        };
        super::choose_chip(&state, n);
        let kept = state.kept();
        assert!(
            kept.len() < all,
            "{} kept {} of {all}, which is not a narrowing",
            chip.title,
            kept.len()
        );
        for session in &kept {
            assert_eq!(
                session.standing,
                standing,
                "{} is kept by {} and is not {:?}",
                session.id,
                chip.title,
                standing.label()
            );
        }
        assert!(
            kept.iter().any(|s| s.id == state.selected.get()),
            "★ {} filtered the picked session away and the detail still describes it",
            chip.title,
        );
    }
}

/// ★★★★★ R1948 — **the crossing action asks for the peer of the session that
/// is showing.**
///
/// The first cross-section action in this application. It is a REQUEST — the
/// section publishes it and the host acts — so what is asserted is that the
/// request names the right peer and is cleared once taken.
#[test]
fn r1948_the_crossing_action_asks_for_the_picked_sessions_peer() {
    let state = fresh();
    assert!(
        super::crossing_request().is_none(),
        "the section opens asking for nothing",
    );
    for session in spec::SESSIONS {
        super::select_session(&state, session.id);
        super::cross_to_topology(&state);
        assert_eq!(
            super::crossing_request().as_deref(),
            Some(session.peer),
            "crossing from {} asked for the wrong peer",
            session.id,
        );
    }
    assert!(
        super::take_crossing().is_some(),
        "the request is taken once"
    );
    assert!(
        super::crossing_request().is_none(),
        "and cleared, so a host cannot act on one request twice",
    );
}

/// ★★★★★ R1948 — **the refusing action names the requirement that books it.**
#[test]
fn r1948_the_refused_action_names_its_requirement() {
    let state = fresh();
    super::refuse_close(&state);
    let said = state.said_sentence();
    assert!(
        said.contains(spec::CLOSE_RESERVED_FOR),
        "the refusal {said:?} names no requirement",
    );
}

/// ★★★★★ R1948 — **the arrows reach every kept session and wrap.**
#[test]
fn r1948_the_arrows_reach_every_kept_session_and_wrap() {
    let state = fresh();
    let kept: Vec<&str> = state.kept().iter().map(|s| s.id).collect();
    let mut seen = vec![state.selected.get()];
    for _ in 1..kept.len() {
        assert!(
            super::key_at(&state, "ArrowDown"),
            "the arrow moved nothing"
        );
        seen.push(state.selected.get());
    }
    for id in &kept {
        assert!(
            seen.iter().any(|s| s == id),
            "{id} is not reachable from the keyboard",
        );
    }
    assert!(super::key_at(&state, "ArrowDown"), "the walk wraps");
    assert_eq!(state.selected.get(), spec::OPENS_ON);
}

/// ★★★★★ R1948 — **every described mark is one the pointer can rest on.**
#[test]
fn r1948_every_description_belongs_to_a_mark_the_pointer_can_rest_on() {
    let described = super::descriptions();
    let tags: Vec<String> = described.tags().map(str::to_owned).collect();
    assert!(!tags.is_empty(), "the register describes nothing");
    for tag in &tags {
        assert_ne!(
            Hit::of_tag(tag),
            Hit::Nothing,
            "{tag} is described and nothing resolves it",
        );
    }
}

/// ★★★★★ R1948 — **the pin and this build name the same parts, in order.**
#[test]
fn r1948_the_specified_parts_are_the_parts_this_build_tables() {
    let document = spec::document();
    for (surface, table) in [("list", spec::LIST), ("detail", spec::DETAIL)] {
        let canon = document
            .canon(surface)
            .unwrap_or_else(|| panic!("the pin declares no surface {surface}"));
        let specified: Vec<&str> = canon.parts().iter().map(|p| p.key.as_ref()).collect();
        let tabled: Vec<&str> = table.iter().map(|p| p.key).collect();
        assert_eq!(
            tabled, specified,
            "{surface}'s tabled parts are not the pin's, in order",
        );
        for part in canon.parts() {
            let ours = table
                .iter()
                .find(|p| p.key == part.key)
                .expect("the keys were just compared");
            assert_eq!(
                ours.title, part.title,
                "{surface}.{} is called two things",
                part.key
            );
        }
    }
}

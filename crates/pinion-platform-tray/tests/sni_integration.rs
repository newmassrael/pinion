//! R950 §3 §5.53 — real-D-Bus integration test for [`SniTrayBackend`].
//!
//! `#[ignore]` by default: it needs a live D-Bus **session bus** with a
//! `StatusNotifierWatcher` (a desktop panel / our own test host), so it is not
//! part of the portable `cargo test --workspace` gate (which must pass on a
//! bus-less CI box) — exactly the live-pixel / x11grab demo convention. Run it
//! where a bus is present:
//!
//! ```text
//! cargo test -p pinion-platform-tray -- --ignored
//! ```
//!
//! It proves the R949 `TrayBackend` trait surface holds against a *real*
//! `StatusNotifierItem` over D-Bus (the second consumer that validates the
//! trait): the service registers an SNI well-known name, the item serves the
//! published `Title` over `org.kde.StatusNotifierItem`, and a live `publish`
//! round-trips.

use std::time::{Duration, Instant};

use pinion_core::tray::{TrayBackend, TrayMenuItem, TrayModel, TrayStatus};
use pinion_platform_tray::SniTrayBackend;

/// The SNI well-known name `ksni` owns for *this* process, if registered yet
/// (`org.kde.StatusNotifierItem-<pid>-<n>`). Format-independent of how a given
/// watcher stores its registry — we look at the bus's own name list.
fn sni_name_for_this_process(conn: &zbus::blocking::Connection) -> Option<String> {
    let pid = std::process::id().to_string();
    let proxy = zbus::blocking::fdo::DBusProxy::new(conn).ok()?;
    proxy
        .list_names()
        .ok()?
        .into_iter()
        .map(|n| n.as_str().to_owned())
        .find(|n| n.starts_with("org.kde.StatusNotifierItem-") && n.contains(&pid))
}

#[test]
#[ignore = "needs a D-Bus session bus + StatusNotifierWatcher; run with --ignored"]
fn sni_backend_registers_and_serves_the_model() {
    let model = TrayModel::new("pinion.test-tray", "Pinion Test", "applications-games")
        .with_status(TrayStatus::Active)
        .with_menu(vec![
            TrayMenuItem::action("hello", "Hello"),
            TrayMenuItem::check("flag", "Flag", true),
        ]);

    let backend = SniTrayBackend::new(&model).expect("SNI service starts on the session bus");
    assert!(backend.is_available(), "a StatusNotifierWatcher is present on this bus");

    let conn = zbus::blocking::Connection::session().expect("session bus");

    // The SNI registration runs async on the service thread — poll (no fixed
    // sleep contract) until our well-known name owns the bus.
    let deadline = Instant::now() + Duration::from_secs(8);
    let item_name = loop {
        if let Some(name) = sni_name_for_this_process(&conn) {
            break name;
        }
        assert!(Instant::now() < deadline, "the SNI item registered a well-known name");
        std::thread::sleep(Duration::from_millis(50));
    };

    // The registered item serves the published Title over the real
    // `org.kde.StatusNotifierItem` interface — the trait surface holds.
    let item = zbus::blocking::Proxy::new(
        &conn,
        item_name.as_str(),
        "/StatusNotifierItem",
        "org.kde.StatusNotifierItem",
    )
    .expect("item proxy");
    let title: String = item.get_property("Title").expect("Title property");
    assert_eq!(title, "Pinion Test", "the item serves the published title");

    // A live model update round-trips over real D-Bus.
    let updated = model.with_status(TrayStatus::NeedsAttention);
    assert!(backend.publish(&updated).is_ok(), "publish updates the live item");
    // No icon / menu clicks were issued, so no events are queued.
    assert!(backend.poll_events().is_empty(), "no spurious events");
}

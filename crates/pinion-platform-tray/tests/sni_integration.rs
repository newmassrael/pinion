//! R1267 §3 §5.53 — headless integration test for [`SniTrayBackend`], driven
//! against an in-process `org.kde.StatusNotifierWatcher` **test-double** on a
//! private D-Bus session bus. It needs no desktop panel and runs inside the
//! portable `cargo test --workspace` gate — closing the "SNI path has zero CI
//! coverage" gap the earlier single `#[ignore]`d test left open.
//!
//! ## Why the re-exec under `dbus-run-session`
//!
//! `ksni` connects only to the ambient session bus (`zbus …
//! connection::Builder::session()`, no explicit-address API), and the
//! workspace sets `unsafe_code = "forbid"`, so the test cannot point it at an
//! isolated bus by mutating `DBUS_SESSION_BUS_ADDRESS` in-process
//! (`std::env::set_var` is `unsafe` on edition 2024). Instead the
//! [`sni_fixture_drives_registration_under_private_bus`] launcher — a normal,
//! gate-run test — re-execs *this* test binary's ignored inner scenario under
//! `dbus-run-session`, which provisions a fresh private bus in the child's
//! environment (child env, not an in-process mutation). The launcher SKIPS,
//! leaving the gate green, when `dbus-run-session` is absent (the bus-less-CI
//! box); the inner scenario stays `#[ignore]`d purely as that re-exec entry
//! point.
//!
//! ## What the scenario proves
//!
//! One child process hosts the watcher double AND the real [`SniTrayBackend`]
//! on the private bus and asserts the full SNI handshake: the item registers
//! with the watcher, serves its `Title` over `org.kde.StatusNotifierItem`,
//! exports its menu over `com.canonical.dbusmenu` at `/MenuBar`, round-trips a
//! live `publish`, and queues no spurious events — the R949 `TrayBackend`
//! trait surface against a real bus, ZERO-FLAKE (private bus + deadline polls,
//! no panel pixels).

use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pinion_core::tray::{TrayBackend, TrayMenuItem, TrayModel, TrayStatus};
use pinion_platform_tray::SniTrayBackend;
use zbus::zvariant::OwnedValue;

/// Env marker the launcher sets on the re-exec'd child so the ignored inner
/// scenario runs its real work only when driven under the private bus (a bare
/// `--ignored` invocation with no bus is then a no-op, never a flake).
const INNER_MARKER: &str = "PINION_TRAY_FIXTURE_INNER";

/// The exact inner-scenario test name the launcher re-execs.
const INNER_TEST: &str = "sni_backend_registers_serves_and_exports_menu";

/// Registered-item names, shared between the watcher double (driven on the zbus
/// object-server thread) and the test thread that polls for the registration.
type Registry = Arc<Mutex<Vec<String>>>;

/// One `com.canonical.dbusmenu` layout node — `(id, a{sv} props, av children)`
/// — as returned by `GetLayout`. Children stay `OwnedValue` (each a variant
/// wrapping a nested node); the fixture asserts on their count, not their
/// nested contents, which keeps the readback robust across zvariant versions.
type MenuLayout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

/// A minimal in-process `org.kde.StatusNotifierWatcher`: only the surface
/// `ksni` drives to register — `RegisterStatusNotifierItem` (recorded) and
/// `IsStatusNotifierHostRegistered == true` (without which `ksni` reports
/// `WontShow` and never registers). No panel, no rendering.
struct WatcherDouble {
    registered: Registry,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl WatcherDouble {
    /// `ksni` calls this to register its item; we record the service name so
    /// the test can assert the handshake happened. This plus
    /// [`Self::is_status_notifier_host_registered`] is the entire surface
    /// `ksni` drives — the rest of the real watcher interface is deliberately
    /// unimplemented (no unconsumed test-double surface).
    fn register_status_notifier_item(&self, service: String) {
        if let Ok(mut items) = self.registered.lock() {
            items.push(service);
        }
    }

    /// `ksni` reads this during registration and bails with `WontShow` unless
    /// it is `true`. Takes `&self` per the zbus interface contract even though
    /// the double's answer is constant.
    #[zbus(property)]
    #[allow(clippy::unused_self)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }
}

/// The SNI well-known name `ksni` owns for this process once registered
/// (`org.kde.StatusNotifierItem-<pid>-<n>`), read from the bus's own name list.
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

/// Launcher: run the SNI scenario under a fresh private session bus by
/// re-execing this binary's ignored inner test under `dbus-run-session`. Skips
/// (leaving the gate green) when that tool is unavailable — the bus-less-CI
/// contract — and fails loudly if the scenario itself fails.
#[test]
fn sni_fixture_drives_registration_under_private_bus() {
    // Probe whether this environment can provision a private session bus at all
    // (`dbus-run-session -- true`). Only a working probe gates the real
    // assertion, so an environmental bus failure — the tool missing, or a
    // sandbox that cannot start `dbus-daemon` — SKIPS (the bus-less-CI
    // contract), while a genuine scenario regression still fails loudly.
    let can_provision_bus = Command::new("dbus-run-session")
        .args(["--", "true"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !can_provision_bus {
        eprintln!("skipping SNI fixture: no private D-Bus session bus available");
        return;
    }

    let exe = std::env::current_exe().expect("test binary path");
    let status = Command::new("dbus-run-session")
        .arg("--")
        .arg(&exe)
        .args([INNER_TEST, "--exact", "--ignored", "--nocapture"])
        .env(INNER_MARKER, "1")
        .status()
        .expect("dbus-run-session ran the fixture child");
    assert!(
        status.success(),
        "the SNI fixture scenario failed under the private bus"
    );
}

/// The real scenario, run only as the launcher's re-exec child (guarded by
/// [`INNER_MARKER`]) so it never runs bus-less. Hosts the watcher double, then
/// drives a real [`SniTrayBackend`] through the full SNI handshake.
#[test]
#[ignore = "re-exec entry for sni_fixture_drives_registration_under_private_bus"]
fn sni_backend_registers_serves_and_exports_menu() {
    if std::env::var_os(INNER_MARKER).is_none() {
        return;
    }

    // Own the watcher name BEFORE the backend registers — deterministic
    // ordering, so the register call never races a missing watcher.
    let registered: Registry = Arc::new(Mutex::new(Vec::new()));
    let _watcher = zbus::blocking::connection::Builder::session()
        .expect("private session bus from dbus-run-session")
        .name("org.kde.StatusNotifierWatcher")
        .expect("watcher well-known name")
        .serve_at(
            "/StatusNotifierWatcher",
            WatcherDouble {
                registered: Arc::clone(&registered),
            },
        )
        .expect("serve watcher object")
        .build()
        .expect("watcher connection owns the name");

    // The real backend registers its StatusNotifierItem with the watcher.
    let model = TrayModel::new("pinion.test-tray", "Pinion Test", "applications-games")
        .with_status(TrayStatus::Active)
        .with_menu(vec![
            TrayMenuItem::action("hello", "Hello"),
            TrayMenuItem::check("flag", "Flag", true),
        ]);
    let backend = SniTrayBackend::new(&model).expect("SNI service starts on the private bus");
    assert!(
        backend.is_available(),
        "the watcher double owns the StatusNotifierWatcher name"
    );

    // Poll (deadline, no fixed-sleep contract) until the watcher records the
    // registration — the SNI register handshake completed.
    let deadline = Instant::now() + Duration::from_secs(8);
    let registered_name = loop {
        if let Some(name) = registered.lock().ok().and_then(|i| i.first().cloned()) {
            break name;
        }
        assert!(
            Instant::now() < deadline,
            "the item registered with the watcher"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    assert!(
        registered_name.starts_with("org.kde.StatusNotifierItem-"),
        "registered service is an SNI item name: {registered_name}"
    );

    // The registered item serves its Title over the real SNI interface.
    let conn = zbus::blocking::Connection::session().expect("session bus");
    let item_name = sni_name_for_this_process(&conn).expect("item owns its well-known name");
    let item = zbus::blocking::Proxy::new(
        &conn,
        item_name.as_str(),
        "/StatusNotifierItem",
        "org.kde.StatusNotifierItem",
    )
    .expect("item proxy");
    let title: String = item.get_property("Title").expect("Title property");
    assert_eq!(title, "Pinion Test", "the item serves the published title");

    // dbusmenu export: the item advertises its menu at /MenuBar and the
    // com.canonical.dbusmenu interface there serves the two published items.
    let menu_path: zbus::zvariant::OwnedObjectPath =
        item.get_property("Menu").expect("Menu property");
    assert_eq!(
        menu_path.as_str(),
        "/MenuBar",
        "the item points at its dbusmenu object"
    );
    let menu = zbus::blocking::Proxy::new(
        &conn,
        item_name.as_str(),
        "/MenuBar",
        "com.canonical.dbusmenu",
    )
    .expect("dbusmenu proxy");
    // GetLayout(parentId=0, recursionDepth=-1, propertyNames=[]) ->
    // (revision, (id, a{sv}, children)); a served menu returns our two items.
    let (_revision, root): (u32, MenuLayout) = menu
        .call("GetLayout", &(0_i32, -1_i32, Vec::<String>::new()))
        .expect("dbusmenu GetLayout is served");
    assert_eq!(
        root.2.len(),
        2,
        "the exported menu has the two published top-level items"
    );

    // A live publish round-trips and no spurious events are queued.
    assert!(
        backend
            .publish(&model.with_status(TrayStatus::NeedsAttention))
            .is_ok(),
        "publish updates the live item"
    );
    assert!(backend.poll_events().is_empty(), "no spurious events");
}

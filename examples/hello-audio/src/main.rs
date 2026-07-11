//! the-tide **audio** — the introspectable audio graph, driven over the
//! shell.
//!
//! The audio peer of `hello-narrative-walk` / `hello-place-map`, and the
//! consumer of the §5.54 (R1274) audio subsystem. The point it makes is the
//! §2 #7 one: the audio graph is **scene-as-data**. Keys play/stop clips on
//! a shared [`AudioEngine`]; the view renders *what is playing* (label /
//! gain / pan / loop) as plain text; an AI agent reads and drives the exact
//! same graph over RPC through [`AudioEngineExternal`] — audio is not hidden
//! behind an opaque handle.
//!
//! No real sound device is used here (a headless box has none): this
//! demonstrates the graph + drive + introspection. Actual output is the
//! [`pinion_audio::AudioBackend`] seam, exercised headlessly by the crate's
//! `InMemoryAudioBackend` capture test.
//!
//! Run: `cargo run -p hello-audio` — `b` bell, `w` waves, `s` stop all,
//! `Esc` quit.

use std::cell::RefCell;
use std::io::Stdout;
use std::rc::Rc;
use std::sync::Arc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_audio::{AudioClip, AudioEngine, AudioEngineExternal};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::reactive::Owner;
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::{Frame, WidgetCore};
use pinion_tui::ratatui::backend::CrosstermBackend;
use pinion_tui::{TuiRenderer, WidgetViewTui};

/// `Owner::cache` key for the shared engine — the one-Rc SSOT.
const ENGINE_KEY: &str = "the_tide.audio_engine";
/// The External / paint-focus tag.
const TAG: &str = "audio";
/// Engine output rate.
const RATE: u32 = 48_000;

/// The shared audio engine, built once per Owner scope.
fn audio_engine() -> Rc<RefCell<AudioEngine>> {
    Owner::current()
        .expect("audio_engine requires an active Owner scope")
        .cache(ENGINE_KEY, || RefCell::new(AudioEngine::new(RATE)))
}

/// A deterministic sine clip — a stand-in for a WAV asset (decode is tested
/// in the crate; this example is about the graph, not the waveform).
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn sine_clip(freq: f32, secs: f32) -> Arc<AudioClip> {
    let frames = (secs * RATE as f32) as usize;
    let samples: Vec<f32> = (0..frames)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            (t * freq * std::f32::consts::TAU).sin() * 0.5
        })
        .collect();
    AudioClip::new(RATE, 1, samples).shared()
}

/// Navigation/trigger events the keyboard maps to.
#[derive(Clone, Copy)]
enum AudioKey {
    PlayBell,
    PlayWaves,
    StopAll,
}

/// The binding unit type.
struct HelloAudio;

impl WidgetCore for HelloAudio {
    /// Voice count — read back so the shell repaints when it changes.
    type State = u16;
    type Event = AudioKey;

    fn create_external() -> Box<dyn External> {
        Box::new(
            AudioEngineExternal::new(audio_engine())
                .with_clip("bell", sine_clip(880.0, 0.4))
                .with_clip("waves", sine_clip(110.0, 0.8)),
        )
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(scene: &Scene) -> u16 {
        if let Scene::External(node) = scene
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Int(n)) = intro.query("voice_count")
        {
            return u16::try_from(n.clamp(0, i64::from(u16::MAX))).unwrap_or(0);
        }
        0
    }

    fn view(_state: u16, _frame: &Frame) -> Scene {
        let engine = audio_engine();
        let engine = engine.borrow();
        let mut children: Vec<Scene> = Vec::new();
        let mut y = 16u32;

        children.push(text_row(
            format!("the-tide audio — {} voice(s) playing", engine.voice_count()),
            y,
        ));
        y += 32;

        if engine.voice_count() == 0 {
            children.push(text_row("(silence)".to_string(), y));
            y += 24;
        } else {
            for (id, voice) in engine.voices() {
                children.push(text_row(
                    format!(
                        "  #{id} {}  gain {:.2}  pan {:+.2}  {}",
                        voice.label(),
                        voice.gain(),
                        voice.pan(),
                        if voice.looping() { "loop" } else { "one-shot" },
                    ),
                    y,
                ));
                y += 20;
            }
        }

        y += 12;
        children.push(text_row(
            "b bell · w waves · s stop all · Esc quit".to_string(),
            y,
        ));

        let mut root = ContainerNode::default();
        root.rect = Rect::new(0, 0, 640, y + 32);
        root.children = children;
        Scene::Container(root)
    }

    fn event_name(event: AudioKey) -> &'static str {
        // The send verb is the clip name (play) or the reserved `stop_all` —
        // no delimiter, so it never clashes with the `:` send-payload grammar.
        match event {
            AudioKey::PlayBell => "bell",
            AudioKey::PlayWaves => "waves",
            AudioKey::StopAll => "stop_all",
        }
    }

    fn title() -> &'static str {
        "pinion hello-audio — introspectable engine audio (§5.54)"
    }

    fn keybinding(key: &str) -> Option<AudioKey> {
        match key {
            "b" => Some(AudioKey::PlayBell),
            "w" => Some(AudioKey::PlayWaves),
            "s" => Some(AudioKey::StopAll),
            _ => None,
        }
    }
}

fn text_row(content: String, y: u32) -> Scene {
    Scene::Text(TextNode::new(content, Rect::new(16, y, 600, 16)))
}

impl WidgetA11y for HelloAudio {
    fn access_node(_state: &u16, focused: Option<&str>) -> Vec<AccessNode> {
        let mut nodes =
            vec![AccessNode::new(TAG, AriaRole::List).with_name("the-tide audio voices")];
        if let Some(engine) =
            Owner::current().and_then(|o| o.cache_get_by_str::<RefCell<AudioEngine>>(ENGINE_KEY))
        {
            let container_focused = focused == Some(TAG);
            for (id, voice) in engine.borrow().voices() {
                nodes.push(
                    AccessNode::new(format!("{TAG}.voice.{id}"), AriaRole::ListItem)
                        .with_name(voice.label().to_string())
                        .with_focused(container_focused),
                );
            }
        }
        nodes
    }
}

impl WidgetViewTui for HelloAudio {
    type Renderer = TuiRenderer<CrosstermBackend<Stdout>>;
}

fn main() {
    if let Err(e) = pinion_tui::run::<HelloAudio>() {
        eprintln!("hello-audio: shell error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_tui::ShellCoreTui;

    fn paint_text(core: &ShellCoreTui<HelloAudio>) -> String {
        fn walk(scene: &Scene, out: &mut Vec<String>) {
            match scene {
                Scene::Text(t) => out.push(t.content.clone()),
                Scene::Container(c) => c.children.iter().for_each(|ch| walk(ch, out)),
                _ => {}
            }
        }
        let mut text = Vec::new();
        walk(&core.compute_paint_scene(), &mut text);
        text.join("\n")
    }

    #[test]
    fn keys_play_clips_and_the_graph_is_readable() {
        let mut core: ShellCoreTui<HelloAudio> = ShellCoreTui::new();
        assert_eq!(core.cached_state(), &0, "starts silent");
        assert!(paint_text(&core).contains("silence"));

        // `b` plays the bell; the graph shows it and the shell repaints.
        assert!(core.dispatch_key("b", pinion_core::Modifiers::empty()));
        assert_eq!(core.cached_state(), &1);
        assert!(paint_text(&core).contains("bell"));

        // `w` adds the waves voice.
        assert!(core.dispatch_key("w", pinion_core::Modifiers::empty()));
        assert_eq!(core.cached_state(), &2);
        assert!(paint_text(&core).contains("waves"));
    }
}

//! The launcher window: search field, virtualized result list, summon toggle.

use std::cell::RefCell;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use asyar_search::{RankInput, rank_ids};
use futures::{StreamExt, channel::mpsc};
use gpui::{
    App, AppContext, Bounds, Context, Entity, Focusable, FontWeight, KeyBinding, QuitMode,
    ScrollStrategy, Subscription, Task, UniformListScrollHandle, Window, WindowBounds,
    WindowDecorations, WindowHandle, WindowKind, WindowOptions, actions, colors::Colors, div,
    point, prelude::*, px, size, uniform_list,
};
use gpui_ce_elements::editable_text::{
    EditableTextState, StringStorage, TextChanged,
    actions::{DEFAULT_INPUT_CONTEXT, default_bindings},
    text_input,
};

use crate::apps::{self, AppEntry};

const CONTEXT: &str = "Launcher";
const WIDTH: f32 = 680.;
const HEIGHT: f32 = 440.;

actions!(launcher, [SelectPrev, SelectNext, Confirm, Dismiss]);

struct Catalog {
    apps: Vec<AppEntry>,
    rank: Arc<Vec<RankInput>>,
}

impl Catalog {
    fn load() -> Self {
        let mut apps = apps::scan();
        // ASYAR_GPUI_SYNTHETIC=10000 pads the list to stress ranking and rendering.
        let synthetic = std::env::var("ASYAR_GPUI_SYNTHETIC")
            .ok()
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0);
        apps.extend((0..synthetic).map(|i| AppEntry {
            name: format!("Synthetic Item {i:05} lorem ipsum"),
            path: Default::default(),
        }));
        let rank = apps
            .iter()
            .enumerate()
            .map(|(i, app)| RankInput {
                id: i.to_string(),
                title: app.name.clone(),
                subtitle: None,
                keywords: Vec::new(),
            })
            .collect();
        Self {
            apps,
            rank: Arc::new(rank),
        }
    }
}

pub fn run(listener: UnixListener) {
    let process_start = Instant::now();
    gpui_platform::application()
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            let t = Instant::now();
            let catalog = Rc::new(Catalog::load());
            eprintln!(
                "[asyar-gpui] indexed {} entries in {:?}",
                catalog.apps.len(),
                t.elapsed()
            );

            cx.bind_keys(default_bindings().as_keybindings(Some(DEFAULT_INPUT_CONTEXT)));
            // Scoped to the field inside the launcher and registered later, so these
            // win over the field's own Up/Down/Escape handling.
            let in_field = format!("{CONTEXT} > {DEFAULT_INPUT_CONTEXT}");
            cx.bind_keys([
                KeyBinding::new("up", SelectPrev, Some(&in_field)),
                KeyBinding::new("down", SelectNext, Some(&in_field)),
                KeyBinding::new("enter", Confirm, Some(&in_field)),
                KeyBinding::new("escape", Dismiss, Some(&in_field)),
                KeyBinding::new("escape", Dismiss, Some(CONTEXT)),
            ]);

            let current: Rc<RefCell<Option<WindowHandle<Launcher>>>> = Default::default();
            toggle(&current, &catalog, process_start, cx);

            let (tx, mut rx) = mpsc::unbounded::<Instant>();
            std::thread::spawn(move || {
                for mut stream in listener.incoming().flatten() {
                    let at = Instant::now();
                    let _ = stream.read(&mut [0u8; 16]);
                    if tx.unbounded_send(at).is_err() {
                        break;
                    }
                }
            });
            cx.spawn(async move |cx| {
                while let Some(at) = rx.next().await {
                    cx.update(|cx| toggle(&current, &catalog, at, cx));
                }
            })
            .detach();
        });
}

/// Shows or hides the parked window; opens one only on first use or if the
/// window manager destroyed it. Re-creating the GPU surface per summon costs
/// ~100 ms, so the window is kept alive and just unmapped.
fn toggle(
    current: &Rc<RefCell<Option<WindowHandle<Launcher>>>>,
    catalog: &Rc<Catalog>,
    requested_at: Instant,
    cx: &mut App,
) {
    let existing = *current.borrow();
    if let Some(handle) = existing
        && handle
            .update(cx, |launcher, window, cx| {
                launcher.toggle(requested_at, window, cx)
            })
            .is_ok()
    {
        return;
    }
    match open(catalog, requested_at, cx) {
        Ok(handle) => *current.borrow_mut() = Some(handle),
        Err(error) => eprintln!("asyar-gpui: cannot open window: {error:#}"),
    }
    cx.activate(true);
}

fn open(
    catalog: &Rc<Catalog>,
    requested_at: Instant,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<Launcher>> {
    let build = |catalog: Rc<Catalog>| {
        move |window: &mut Window, cx: &mut App| {
            cx.new(|cx| Launcher::new(catalog, requested_at, window, cx))
        }
    };
    let base = || WindowOptions {
        titlebar: None,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some("asyar-gpui".into()),
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        use gpui::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
        let shell = LayerShellOptions::default()
            .namespace("asyar")
            .layer(Layer::Overlay)
            .anchor(Anchor::TOP)
            .margin(Some((px(160.), px(0.), px(0.), px(0.))))
            .keyboard_interactivity(KeyboardInteractivity::Exclusive);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(WIDTH), px(HEIGHT)),
            })),
            kind: WindowKind::LayerShell(shell),
            ..base()
        };
        match cx.open_window(options, build(catalog.clone())) {
            Ok(handle) => return Ok(handle),
            // GNOME has no wlr-layer-shell: fall through to a normal window.
            Err(error) => {
                eprintln!("[asyar-gpui] layer-shell unavailable ({error}); using a window")
            }
        }
    }

    let bounds = Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        kind: WindowKind::Normal,
        ..base()
    };
    cx.open_window(options, build(catalog.clone()))
}

pub struct Launcher {
    catalog: Rc<Catalog>,
    query: Entity<EditableTextState>,
    results: Rc<Vec<usize>>,
    selected: usize,
    scroll: UniformListScrollHandle,
    last_rank: Duration,
    was_active: bool,
    visible: bool,
    rank_task: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl Launcher {
    fn new(
        catalog: Rc<Catalog>,
        requested_at: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        window.focus(&query.focus_handle(cx), cx);
        log_first_frame(window, requested_at);

        let subscriptions = vec![
            cx.subscribe_in(&query, window, |this, _, _: &TextChanged, window, cx| {
                // Not visible means the reset on hide, not a keystroke.
                let typed_at = this.visible.then(Instant::now);
                this.rerank(typed_at, window, cx);
                if let Some(typed_at) = typed_at {
                    window.on_next_frame(move |_, _| {
                        eprintln!("[asyar-gpui] keystroke -> echo {:?}", typed_at.elapsed());
                    });
                }
            }),
            // Launchers dismiss on focus loss; ignore the inactive state before first focus.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.was_active = true;
                } else if this.was_active {
                    this.hide(window, cx);
                }
            }),
        ];

        Self {
            results: Rc::new((0..catalog.apps.len()).collect()),
            catalog,
            query,
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            last_rank: Duration::ZERO,
            was_active: false,
            visible: true,
            rank_task: Task::ready(()),
            _subscriptions: subscriptions,
        }
    }

    fn toggle(&mut self, requested_at: Instant, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible {
            self.hide(window, cx);
        } else {
            self.visible = true;
            window.set_visible(true);
            window.activate();
            window.focus(&self.query.focus_handle(cx), cx);
            log_first_frame(window, requested_at);
            cx.notify();
        }
    }

    fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = false;
        self.was_active = false;
        window.set_visible(false);
        // Next summon starts from an empty query, like every launcher.
        self.query.update(cx, |query, cx| query.emplace("", cx));
    }

    /// Ranks off the UI thread so typing never waits on the ranker; the field
    /// echoes the keystroke immediately and results land a frame later.
    fn rerank(&mut self, typed_at: Option<Instant>, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query.read(cx).as_str().to_owned();
        let items = self.catalog.rank.clone();
        let ranking = cx.background_executor().spawn(async move {
            let t = Instant::now();
            let ids: Vec<usize> = rank_ids(&query, &items)
                .iter()
                .filter_map(|id| id.parse().ok())
                .collect();
            (ids, t.elapsed())
        });
        // Replacing the task drops the pending result of a stale query.
        self.rank_task = cx.spawn_in(window, async move |this, cx| {
            let (results, took) = ranking.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.results = Rc::new(results);
                this.last_rank = took;
                this.select(0, cx);
                if let Some(typed_at) = typed_at {
                    let count = this.results.len();
                    window.on_next_frame(move |_, _| {
                        eprintln!(
                            "[asyar-gpui] keystroke -> results {:?} (rank {took:?}, {count} results)",
                            typed_at.elapsed()
                        );
                    });
                }
            });
        });
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = ix;
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.notify();
    }
}

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::for_appearance(window);
        let selected = self.selected;
        let results = self.results.clone();
        let catalog = self.catalog.clone();
        let count = results.len();

        let rows = uniform_list("results", count, move |range, _window, _cx| {
            range
                .map(|ix| {
                    let row = div()
                        .id(ix)
                        .h(px(40.))
                        .px_4()
                        .flex()
                        .items_center()
                        .rounded_md()
                        .child(catalog.apps[results[ix]].name.clone());
                    if ix == selected {
                        row.bg(colors.selected).text_color(colors.selected_text)
                    } else {
                        row
                    }
                })
                .collect()
        })
        .track_scroll(&self.scroll)
        .flex_1()
        .min_h_0()
        .px_2();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.text)
            .rounded_xl()
            .border_1()
            .border_color(colors.border)
            .overflow_hidden()
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| {
                this.select(this.selected.saturating_sub(1), cx);
            }))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| {
                let last = this.results.len().saturating_sub(1);
                this.select((this.selected + 1).min(last), cx);
            }))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| {
                if let Some(&app) = this.results.get(this.selected) {
                    apps::launch(&this.catalog.apps[app]);
                    this.hide(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| this.hide(window, cx)))
            .child(
                div().p_3().border_b_1().border_color(colors.border).child(
                    text_input("query")
                        .state(self.query.downgrade())
                        .placeholder("Search apps…")
                        .placeholder_color(colors.disabled)
                        .caret_color(gpui::rgb_to_hsla(colors.text))
                        .w_full()
                        .whitespace_nowrap()
                        .px_2()
                        .py_1()
                        .text_lg(),
                ),
            )
            .child(rows)
            .child(
                div()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(colors.border)
                    .text_sm()
                    .text_color(colors.disabled)
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!(
                        "{count} results · ranked in {:.2} ms",
                        self.last_rank.as_secs_f64() * 1000.
                    )),
            )
    }
}

fn log_first_frame(window: &Window, requested_at: Instant) {
    window.on_next_frame(move |_, _| {
        eprintln!(
            "[asyar-gpui] summon -> first frame: {:?}",
            requested_at.elapsed()
        );
    });
}

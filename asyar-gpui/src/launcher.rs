//! The launcher window: search field, virtualized result list, summon toggle.

use std::cell::RefCell;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use asyar_calculator::{CalcResult, EvalContext, evaluate_query, should_attempt};
use asyar_search::{RankInput, rank_ids};
use futures::{StreamExt, channel::mpsc};
use gpui::{
    App, AppContext, Bounds, ClipboardItem, Context, Entity, Focusable, FontWeight, KeyBinding,
    LinuxWindowBackground, ObjectFit, QuitMode, RenderImage, ScrollStrategy, Subscription, Task,
    UniformListScrollHandle, Window, WindowBounds, WindowDecorations, WindowHandle, WindowKind,
    WindowOptions, actions, div, img, linear_color_stop, linear_gradient, point, prelude::*, px,
    size, transparent_black, uniform_list,
};
use gpui_ce_elements::editable_text::{
    EditableTextState, StringStorage, TextChanged,
    actions::{DEFAULT_INPUT_CONTEXT, default_bindings},
    text_input,
};

use crate::apps::{self, AppEntry};
use crate::rates::{Rates, Snapshot};
use crate::settings::{Glass, Settings};

const CONTEXT: &str = "Launcher";
const WIDTH: f32 = 680.;
const HEIGHT: f32 = 440.;
const ROW_HEIGHT: f32 = 44.;
/// Transparent border around the glass panel that holds its drop shadow.
const SHADOW_MARGIN: f32 = 24.;
const PANEL_RADIUS: f32 = 20.;

actions!(launcher, [SelectPrev, SelectNext, Confirm, Dismiss]);

struct Catalog {
    apps: Vec<AppEntry>,
    rank: Arc<Vec<RankInput>>,
    /// Filled in by a background lookup after the first frame; empty until then.
    icons: RefCell<Rc<Vec<Option<PathBuf>>>>,
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
            icon_name: None,
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
            icons: Default::default(),
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

            let names: Vec<Option<String>> = catalog
                .apps
                .iter()
                .map(|app| app.icon_name.clone())
                .collect();
            let lookup = cx.background_executor().spawn(async move {
                let t = Instant::now();
                (apps::resolve_icons(&names), t.elapsed())
            });
            let icon_catalog = catalog.clone();
            cx.spawn(async move |cx| {
                let (icons, took) = lookup.await;
                eprintln!("[asyar-gpui] resolved icons in {took:?}");
                *icon_catalog.icons.borrow_mut() = Rc::new(icons);
                cx.update(|cx| cx.refresh_windows());
            })
            .detach();

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
    // Transparency (rounded corners, shadow, glass) only shows under a
    // compositor; without one the margin would render as a black frame.
    let composited = crate::x11_place::compositing();
    let margin = if composited { SHADOW_MARGIN } else { 0. };
    let window_size = size(px(WIDTH + 2. * margin), px(HEIGHT + 2. * margin));
    let build = |catalog: Rc<Catalog>| {
        move |window: &mut Window, cx: &mut App| {
            cx.new(|cx| Launcher::new(catalog, margin, requested_at, window, cx))
        }
    };
    let base = || WindowOptions {
        // Shown by `Launcher::show` once placed and the backdrop is captured.
        show: false,
        linux_window_background: if !composited {
            LinuxWindowBackground::Opaque
        } else if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            LinuxWindowBackground::Blurred
        } else {
            LinuxWindowBackground::Transparent
        },
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
                size: window_size,
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

    let bounds = Bounds::centered(None, window_size, cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        kind: WindowKind::Normal,
        ..base()
    };
    cx.open_window(options, build(catalog.clone()))
}

/// One result row: calculator answers come first, then ranked apps.
enum Row {
    Calc(CalcResult),
    App(usize),
}

/// Calculator answers for `query`, or none when it can't be a calculation.
/// `rates` is only read for queries that can be calculations.
fn calculate(
    query: &str,
    settings: &Settings,
    rates: impl FnOnce() -> Snapshot,
) -> Vec<CalcResult> {
    if !should_attempt(query) {
        return Vec::new();
    }
    let (rates, rates_age) = rates();
    let ctx = EvalContext::current(
        rates,
        rates_age,
        settings.preferred_currency.clone(),
        settings.number_format,
    );
    evaluate_query(query, &ctx)
}

pub struct Launcher {
    catalog: Rc<Catalog>,
    settings: Settings,
    rates: Arc<Rates>,
    /// Logical px of transparent shadow border around the panel.
    margin: f32,
    /// Blurred snapshot of what sits behind the panel (X11 only).
    backdrop: Option<Arc<RenderImage>>,
    query: Entity<EditableTextState>,
    results: Rc<Vec<Row>>,
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
        margin: f32,
        requested_at: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = Settings::load();
        let query = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));

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

        let mut this = Self {
            results: Rc::new((0..catalog.apps.len()).map(Row::App).collect()),
            catalog,
            rates: {
                let rates = Arc::new(Rates::default());
                rates.ensure_fresh(settings.rates_ttl_hours);
                rates
            },
            settings,
            margin,
            backdrop: None,
            query,
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            last_rank: Duration::ZERO,
            was_active: false,
            visible: false,
            rank_task: Task::ready(()),
            _subscriptions: subscriptions,
        };
        this.show(requested_at, window, cx);
        this
    }

    fn toggle(&mut self, requested_at: Instant, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible {
            self.hide(window, cx);
        } else {
            self.show(requested_at, window, cx);
        }
    }

    fn show(&mut self, requested_at: Instant, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = true;
        // Picks up changes made in the Tauri app's Settings window.
        self.settings = Settings::load();
        self.rates.ensure_fresh(self.settings.rates_ttl_hours);
        // Placed and captured while unmapped: the WM applies the position on
        // map, and the capture can't contain the launcher itself.
        let placed = crate::x11_place::center_on_pointer_monitor(window);
        let t = Instant::now();
        let inset = (self.margin * window.scale_factor()).round() as i32;
        self.backdrop = placed.and_then(|rect| crate::backdrop::capture(rect.inset(inset)));
        if self.backdrop.is_some() {
            eprintln!("[asyar-gpui] backdrop captured in {:?}", t.elapsed());
        }
        window.set_visible(true);
        window.activate();
        window.focus(&self.query.focus_handle(cx), cx);
        log_first_frame(window, requested_at);
        cx.notify();
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
        let settings = self.settings.clone();
        let rates = self.rates.clone();
        let ranking = cx.background_executor().spawn(async move {
            let t = Instant::now();
            let mut rows: Vec<Row> = calculate(&query, &settings, || rates.snapshot())
                .into_iter()
                .map(Row::Calc)
                .collect();
            rows.extend(
                rank_ids(&query, &items)
                    .iter()
                    .filter_map(|id| id.parse().ok())
                    .map(Row::App),
            );
            (rows, t.elapsed())
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
        let colors = Glass::new(
            self.settings.is_dark(window),
            self.backdrop.is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some(),
        );
        let selected = self.selected;
        let results = self.results.clone();
        let catalog = self.catalog.clone();
        let icons = self.catalog.icons.borrow().clone();
        let count = results.len();

        let rows = uniform_list("results", count, move |range, _window, _cx| {
            range
                .map(|ix| {
                    let row = div()
                        .id(ix)
                        .w_full()
                        .h(px(ROW_HEIGHT))
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_3()
                        .rounded_lg();
                    let row = match &results[ix] {
                        Row::App(app) => {
                            let icon = match icons.get(*app) {
                                Some(Some(path)) => {
                                    img(path.clone()).size(px(24.)).into_any_element()
                                }
                                _ => div().size(px(24.)).into_any_element(),
                            };
                            row.child(icon).child(catalog.apps[*app].name.clone())
                        }
                        Row::Calc(calc) => row
                            .child(div().size(px(24.)).flex().justify_center().child("="))
                            .child(
                                div()
                                    .flex_1()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(calc.value.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(colors.muted)
                                    .child(calc.detail.clone()),
                            ),
                    };
                    if ix == selected {
                        row.bg(colors.selection)
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

        let radius = px(PANEL_RADIUS);
        let layer = || div().absolute().inset_0().rounded(radius);
        let content = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .text_color(colors.text)
            .child(
                div().p_3().border_b_1().border_color(colors.divider).child(
                    text_input("query")
                        .state(self.query.downgrade())
                        .placeholder("Search apps…")
                        .placeholder_color(colors.muted)
                        .caret_color(colors.text)
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
                    .border_color(colors.divider)
                    .text_sm()
                    .text_color(colors.muted)
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!(
                        "{count} results · ranked in {:.2} ms",
                        self.last_rank.as_secs_f64() * 1000.
                    )),
            );

        // Glass, bottom to top: blurred backdrop, tint, a sheen fading down from
        // the top edge, then content; a bright rim and a soft drop shadow outside.
        let panel = div()
            .relative()
            .size_full()
            .rounded(radius)
            .border_1()
            .border_color(colors.rim)
            .when(self.margin > 0., |panel| panel.shadow_xl())
            .children(self.backdrop.clone().map(|image| {
                img(image)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .rounded(radius)
                    .object_fit(ObjectFit::Fill)
            }))
            .child(layer().bg(colors.tint))
            .child(layer().bg(linear_gradient(
                180.,
                linear_color_stop(colors.sheen, 0.),
                linear_color_stop(transparent_black(), 0.4),
            )))
            .child(content);

        div()
            .size_full()
            .p(px(self.margin))
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| {
                this.select(this.selected.saturating_sub(1), cx);
            }))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| {
                let last = this.results.len().saturating_sub(1);
                this.select((this.selected + 1).min(last), cx);
            }))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| {
                match this.results.get(this.selected) {
                    Some(Row::App(app)) => apps::launch(&this.catalog.apps[*app]),
                    Some(Row::Calc(calc)) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(calc.value.clone()))
                    }
                    None => return,
                }
                this.hide(window, cx);
            }))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| this.hide(window, cx)))
            .child(panel)
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

#[cfg(test)]
mod tests {
    use super::calculate;
    use crate::settings::{Settings, Theme};
    use asyar_calculator::locale::NumberFormat;

    fn settings() -> Settings {
        Settings {
            theme: Theme::System,
            number_format: NumberFormat::Point,
            preferred_currency: "USD".into(),
            rates_ttl_hours: 6.0,
        }
    }

    #[test]
    fn arithmetic_queries_get_an_answer_and_app_names_do_not() {
        assert_eq!(
            calculate("3+3", &settings(), || (None, None))
                .first()
                .map(|r| r.value.as_str()),
            Some("6")
        );
        assert!(calculate("firefox", &settings(), || (None, None)).is_empty());
    }

    #[test]
    fn currency_queries_use_the_supplied_rates() {
        let rates = [("USD".to_owned(), 1.0), ("EUR".to_owned(), 0.5)];
        let snapshot = || {
            (
                Some(std::sync::Arc::new(rates.into_iter().collect())),
                Some("1 h".to_owned()),
            )
        };
        let answer = calculate("10 usd in eur", &settings(), snapshot);
        assert!(
            answer.first().is_some_and(|r| r.value.contains('5')),
            "{answer:?}"
        );
    }
}

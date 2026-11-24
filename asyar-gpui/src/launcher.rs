//! The launcher window: search field, virtualized result list, summon toggle.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use asyar_calculator::{CalcResult, EvalContext, evaluate_query, should_attempt};
use asyar_search::{RankInput, rank_ids};
use futures::{
    StreamExt,
    channel::{mpsc, oneshot},
};
use gpui::component::input::{Input, InputEvent, InputState};
use gpui::component::{Root, Theme, ThemeMode};
use gpui::{
    App, AppContext, Bounds, ClipboardItem, Context, Entity, Focusable, FontWeight, KeyBinding,
    ObjectFit, QuitMode, RenderImage, ScrollStrategy, Subscription, Task, UniformListScrollHandle,
    Window, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowHandle, WindowKind,
    WindowOptions, actions, div, img, linear_color_stop, linear_gradient, point, prelude::*, px,
    size, transparent_black, uniform_list,
};

use crate::app::{self, AppEvent, ToolApp};
use crate::apps::{self, AppEntry};
use crate::plugins::{self, Discovered, Item, PluginCommand};
use crate::rates::{Rates, Snapshot};
use crate::settings::{Glass, Settings};

const CONTEXT: &str = "Launcher";
const PLACEHOLDER: &str = "Search apps and commands…";
const WIDTH: f32 = 860.;
const HEIGHT: f32 = 540.;
const ROW_HEIGHT: f32 = 44.;
/// gpui-kit's client-side window frame around the panel: 20 px of
/// transparent shadow padding plus a 1 px border.
const FRAME: f32 = 21.;
const PANEL_RADIUS: f32 = 20.;
/// How long focus must stay lost before the launcher hides.
const BLUR_SETTLE: Duration = Duration::from_millis(150);

actions!(launcher, [SelectPrev, SelectNext, Confirm, Dismiss]);

struct Catalog {
    apps: Vec<AppEntry>,
    app_rank: Vec<RankInput>,
    /// Apps, then plugin commands (`p<index>`) and tool app entries (`t<index>`).
    rank: RefCell<Arc<Vec<RankInput>>>,
    plugins: RefCell<Arc<Vec<Arc<PluginCommand>>>>,
    tools: RefCell<Arc<Vec<Arc<ToolEntry>>>>,
    plugins_seen: RefCell<Option<plugins::Fingerprint>>,
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
        let app_rank: Vec<RankInput> = apps
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
            rank: RefCell::new(Arc::new(app_rank.clone())),
            app_rank,
            plugins: Default::default(),
            tools: Default::default(),
            plugins_seen: Default::default(),
            icons: Default::default(),
        }
    }

    fn set_plugins(&self, found: Discovered) {
        let commands: Vec<Arc<PluginCommand>> = found.commands.into_iter().map(Arc::new).collect();
        let tools: Vec<Arc<ToolEntry>> = found
            .apps
            .into_iter()
            .flat_map(|app| {
                let title = app.manifest.title.clone();
                app.manifest.entries.into_iter().map(move |entry| {
                    Arc::new(ToolEntry {
                        program: app.program.clone(),
                        title: title.clone(),
                        entry,
                    })
                })
            })
            .collect();
        let mut rank = self.app_rank.clone();
        rank.extend(commands.iter().enumerate().map(|(i, c)| RankInput {
            id: format!("p{i}"),
            title: c.title(),
            subtitle: Some(c.summary.clone()),
            keywords: vec![c.plugin.clone()],
        }));
        rank.extend(tools.iter().enumerate().map(|(i, t)| RankInput {
            id: format!("t{i}"),
            title: t.entry.title.clone(),
            subtitle: Some(t.entry.subtitle.clone()),
            keywords: t.entry.keywords.clone(),
        }));
        *self.rank.borrow_mut() = Arc::new(rank);
        *self.plugins.borrow_mut() = Arc::new(commands);
        *self.tools.borrow_mut() = Arc::new(tools);
    }
}

/// A root-search entry that opens a tool's native app (`asyar-ui`).
struct ToolEntry {
    program: PathBuf,
    /// The tool's own name, shown in the app header ("CYL").
    title: String,
    entry: crate::ui::Entry,
}

/// Re-reads plugin catalogs off the UI thread, only when a manifest or a
/// plugin program changed since the last summon.
fn refresh_plugins(catalog: &Rc<Catalog>, cx: &mut App) {
    let Some(dir) = plugins::dir() else {
        return;
    };
    let print = plugins::fingerprint(&dir);
    if catalog.plugins_seen.borrow().as_ref() == Some(&print) {
        return;
    }
    *catalog.plugins_seen.borrow_mut() = Some(print);
    let (tx, rx) = oneshot::channel();
    std::thread::spawn(move || {
        let t = Instant::now();
        let _ = tx.send((plugins::discover(&dir), t.elapsed()));
    });
    let catalog = catalog.clone();
    cx.spawn(async move |cx| {
        if let Ok((commands, took)) = rx.await {
            eprintln!(
                "[asyar-gpui] loaded {} plugin commands and {} app entries in {took:?}",
                commands.commands.len(),
                commands
                    .apps
                    .iter()
                    .map(|a| a.manifest.entries.len())
                    .sum::<usize>()
            );
            catalog.set_plugins(commands);
            cx.update(|cx| cx.refresh_windows());
        }
    })
    .detach();
}

pub fn run(listener: UnixListener) {
    let process_start = Instant::now();
    gpui::application()
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            gpui::init(cx);
            let t = Instant::now();
            let catalog = Rc::new(Catalog::load());
            eprintln!(
                "[asyar-gpui] indexed {} entries in {:?}",
                catalog.apps.len(),
                t.elapsed()
            );

            // Scoped to the field inside the launcher and registered after
            // `gpui::init`, so these win over the field's own Up/Down/Enter/Escape.
            let in_field = format!("{CONTEXT} > Input");
            cx.bind_keys([
                KeyBinding::new("up", SelectPrev, Some(&in_field)),
                KeyBinding::new("down", SelectNext, Some(&in_field)),
                KeyBinding::new("enter", Confirm, Some(&in_field)),
                KeyBinding::new("escape", Dismiss, Some(&in_field)),
                KeyBinding::new("escape", Dismiss, Some(CONTEXT)),
            ]);
            // A tool app's field: registered last so these win inside apps.
            let in_app = format!("{} > Input", app::CONTEXT);
            cx.bind_keys([
                KeyBinding::new("up", app::Up, Some(&in_app)),
                KeyBinding::new("down", app::Down, Some(&in_app)),
                KeyBinding::new("enter", app::Confirm, Some(&in_app)),
                KeyBinding::new("escape", app::Back, Some(&in_app)),
                KeyBinding::new("ctrl-k", app::ShowActions, Some(&in_app)),
                KeyBinding::new("ctrl-tab", app::NextTab, Some(&in_app)),
                KeyBinding::new("ctrl-shift-tab", app::PrevTab, Some(&in_app)),
            ]);

            let current: Rc<RefCell<Option<Shown>>> = Default::default();
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
            grab_hotkey(tx.clone());
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

/// On Wayland, bind the `asyar-gpui` binary to a compositor shortcut instead.
fn grab_hotkey(tx: mpsc::UnboundedSender<Instant>) {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return;
    }
    let spec = crate::settings::hotkey();
    match crate::hotkey::start(&spec, move || {
        let _ = tx.unbounded_send(Instant::now());
    }) {
        Ok(()) => eprintln!("[asyar-gpui] hotkey {spec}"),
        Err(error) => eprintln!("[asyar-gpui] cannot grab hotkey {spec:?}: {error}"),
    }
}

/// The launcher window: gpui-kit's `Root` wraps the `Launcher` view.
type Shown = (WindowHandle<Root>, Entity<Launcher>);

/// Shows or hides the parked window; opens one only on first use or if the
/// window manager destroyed it. Re-creating the GPU surface per summon costs
/// ~100 ms, so the window is kept alive and just unmapped.
fn toggle(
    current: &Rc<RefCell<Option<Shown>>>,
    catalog: &Rc<Catalog>,
    requested_at: Instant,
    cx: &mut App,
) {
    let existing = current.borrow().clone();
    if let Some((handle, launcher)) = existing
        && handle
            .update(cx, |_, window, cx| {
                launcher.update(cx, |launcher, cx| launcher.toggle(requested_at, window, cx))
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

fn open(catalog: &Rc<Catalog>, requested_at: Instant, cx: &mut App) -> anyhow::Result<Shown> {
    // Transparency (rounded corners, shadow, glass) only shows under a
    // compositor.
    // shortcut: without one, gpui-kit's frame padding renders as a black
    // border; fine on every composited desktop, revisit for bare X11.
    let composited = crate::x11_place::compositing();
    let window_size = size(px(WIDTH + 2. * FRAME), px(HEIGHT + 2. * FRAME));
    let launcher: Rc<RefCell<Option<Entity<Launcher>>>> = Default::default();
    let build = |catalog: Rc<Catalog>| {
        let launcher = launcher.clone();
        move |window: &mut Window, cx: &mut App| {
            let view = cx.new(|cx| Launcher::new(catalog, requested_at, window, cx));
            *launcher.borrow_mut() = Some(view.clone());
            // Root paints the theme background by default; the panel draws its own.
            cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
        }
    };
    let shown = |handle: WindowHandle<Root>| {
        let view = launcher.borrow_mut().take().expect("built with the window");
        (handle, view)
    };
    let base = || WindowOptions {
        window_background: if !composited {
            WindowBackgroundAppearance::Opaque
        } else if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Transparent
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
        let shell = LayerShellOptions {
            namespace: "asyar".into(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP,
            margin: Some((px(160.), px(0.), px(0.), px(0.))),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            ..Default::default()
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(0.), px(0.)),
                size: window_size,
            })),
            kind: WindowKind::LayerShell(shell),
            ..base()
        };
        match cx.open_window(options, build(catalog.clone())) {
            Ok(handle) => return Ok(shown(handle)),
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
    Ok(shown(cx.open_window(options, build(catalog.clone()))?))
}

/// One result row: calculator answers come first, then ranked apps and
/// plugin commands; inside a plugin view, that command's items.
enum Row {
    Calc(CalcResult),
    App(usize),
    Plugin(Arc<PluginCommand>),
    Item(usize),
    Tool(Arc<ToolEntry>),
}

/// A plugin command's result list, replacing the search results.
struct View {
    command: Arc<PluginCommand>,
    items: Rc<Vec<Item>>,
    state: ViewState,
}

enum ViewState {
    Loading,
    Ready,
    Failed(String),
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
    /// Blurred snapshot of what sits behind the panel (X11 only).
    backdrop: Option<Arc<RenderImage>>,
    query: Entity<InputState>,
    results: Rc<Vec<Row>>,
    selected: usize,
    scroll: UniformListScrollHandle,
    last_rank: Duration,
    was_active: bool,
    visible: bool,
    /// When focus loss last hid the window (see `toggle`).
    blurred_at: Option<Instant>,
    /// Pending hide after a focus loss; dropped if focus comes back.
    blur_check: Task<()>,
    rank_task: Task<()>,
    view: Option<View>,
    /// Last result per command, shown at once while a fresh run loads.
    cache: HashMap<String, Rc<Vec<Item>>>,
    run_task: Task<()>,
    /// An open tool app replaces the search content until Esc.
    app: Option<(Entity<ToolApp>, Subscription)>,
    _subscriptions: Vec<Subscription>,
}

impl Launcher {
    fn new(
        catalog: Rc<Catalog>,
        requested_at: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = Settings::load();
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(PLACEHOLDER));

        let subscriptions = vec![
            cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
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
                    this.blur_check = Task::ready(());
                } else if this.was_active {
                    // GPUI's X11 backend reports every FocusOut, including the
                    // brief ones a keyboard grab causes when the hotkey is
                    // released; only a focus loss that lasts is a dismissal.
                    this.blur_check = cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(BLUR_SETTLE).await;
                        let _ = this.update_in(cx, |this, window, cx| {
                            if this.visible && !window.is_window_active() {
                                this.hide(window, cx);
                                this.blurred_at = Some(Instant::now());
                            }
                        });
                    });
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
            backdrop: None,
            query,
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            last_rank: Duration::ZERO,
            was_active: false,
            visible: false,
            blurred_at: None,
            blur_check: Task::ready(()),
            rank_task: Task::ready(()),
            view: None,
            cache: HashMap::new(),
            run_task: Task::ready(()),
            app: None,
            _subscriptions: subscriptions,
        };
        // GPUI maps every new window; unmap it so `show` can place it and
        // capture the backdrop first.
        crate::x11_place::set_mapped(window, false);
        this.show(requested_at, window, cx);
        this
    }

    fn toggle(&mut self, requested_at: Instant, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible {
            self.hide(window, cx);
        } else if !hidden_by_same_press(self.blurred_at, requested_at) {
            self.show(requested_at, window, cx);
        }
    }

    fn show(&mut self, requested_at: Instant, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = true;
        // Picks up changes made in the Tauri app's Settings window.
        self.settings = Settings::load();
        self.rates.ensure_fresh(self.settings.rates_ttl_hours);
        refresh_plugins(&self.catalog, cx);
        // gpui-kit components (the search field) and its window frame follow
        // the launcher's light or dark glass.
        let dark = self.settings.is_dark(window);
        Theme::change(
            if dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            None,
            cx,
        );
        let theme = Theme::global_mut(cx);
        theme.window_border = Glass::new(dark, true).rim;
        theme.window_radius = px(PANEL_RADIUS);
        // Placed and captured while unmapped: the WM applies the position on
        // map, and the capture can't contain the launcher itself.
        let placed = crate::x11_place::center_on_pointer_monitor(window);
        let t = Instant::now();
        let inset = (FRAME * window.scale_factor()).round() as i32;
        self.backdrop = placed.and_then(|rect| crate::backdrop::capture(rect.inset(inset)));
        if self.backdrop.is_some() {
            eprintln!("[asyar-gpui] backdrop captured in {:?}", t.elapsed());
        }
        crate::x11_place::set_mapped(window, true);
        window.activate_window();
        window.focus(&self.query.focus_handle(cx), cx);
        log_first_frame(window, requested_at);
        cx.notify();
    }

    fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = false;
        self.was_active = false;
        self.view = None;
        self.app = None;
        self.run_task = Task::ready(());
        // Next summon starts from an empty query, like every launcher.
        self.set_query("", PLACEHOLDER, window, cx);
        // shortcut: Wayland can't unmap, so the window closes and the next
        // summon re-creates it (~100 ms); keep it if GPUI gains show/hide.
        if !crate::x11_place::set_mapped(window, false) {
            window.remove_window();
        }
    }

    fn set_query(
        &mut self,
        text: &str,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = placeholder.to_owned();
        self.query.update(cx, |query, cx| {
            query.set_placeholder(placeholder, window, cx);
            query.set_value(text.to_owned(), window, cx);
        });
    }

    /// Ranks off the UI thread so typing never waits on the ranker; the field
    /// echoes the keystroke immediately and results land a frame later.
    fn rerank(&mut self, typed_at: Option<Instant>, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().to_string();
        // Plugin items are already in memory: filter them here, no ranking task.
        if let Some(view) = &self.view {
            self.rank_task = Task::ready(());
            self.results = Rc::new(
                plugins::filter(&query, &view.items)
                    .into_iter()
                    .map(Row::Item)
                    .collect(),
            );
            self.select(0, cx);
            return;
        }
        let items = self.catalog.rank.borrow().clone();
        let commands = self.catalog.plugins.borrow().clone();
        let tools = self.catalog.tools.borrow().clone();
        let settings = self.settings.clone();
        let rates = self.rates.clone();
        let ranking = cx.background_executor().spawn(async move {
            let t = Instant::now();
            let mut rows: Vec<Row> = calculate(&query, &settings, || rates.snapshot())
                .into_iter()
                .map(Row::Calc)
                .collect();
            rows.extend(rank_ids(&query, &items).iter().filter_map(|id| {
                if let Some(ix) = id.strip_prefix('p') {
                    commands
                        .get(ix.parse::<usize>().ok()?)
                        .cloned()
                        .map(Row::Plugin)
                } else if let Some(ix) = id.strip_prefix('t') {
                    tools.get(ix.parse::<usize>().ok()?).cloned().map(Row::Tool)
                } else {
                    id.parse().ok().map(Row::App)
                }
            }));
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

    /// Runs `command` in its own thread (it may take seconds on the network)
    /// and lists its items; cached items show meanwhile.
    fn open_view(
        &mut self,
        command: Arc<PluginCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = command.title();
        self.view = Some(View {
            command: command.clone(),
            items: self.cache.get(&title).cloned().unwrap_or_default(),
            state: ViewState::Loading,
        });
        self.set_query("", &format!("Filter {title}…"), window, cx);
        self.rerank(None, window, cx);

        let (tx, rx) = oneshot::channel();
        let job = command.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job.run());
        });
        self.run_task = cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                // Ignore a run that finished after its view was left.
                let Some(view) = this
                    .view
                    .as_mut()
                    .filter(|view| Arc::ptr_eq(&view.command, &command))
                else {
                    return;
                };
                match result {
                    Ok(items) => {
                        let items = Rc::new(items);
                        this.cache.insert(title, items.clone());
                        view.items = items;
                        view.state = ViewState::Ready;
                    }
                    Err(error) => view.state = ViewState::Failed(error),
                }
                this.rerank(None, window, cx);
            });
        });
    }

    fn open_app(&mut self, tool: Arc<ToolEntry>, window: &mut Window, cx: &mut Context<Self>) {
        let looks = (
            self.settings.is_dark(window),
            self.backdrop.is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some(),
        );
        let app = cx.new(|cx| {
            ToolApp::new(
                tool.program.clone(),
                tool.title.clone(),
                tool.entry.route.clone(),
                looks,
                window,
                cx,
            )
        });
        let subscription =
            cx.subscribe_in(&app, window, |this, _, event, window, cx| match event {
                AppEvent::Close => {
                    this.app = None;
                    window.focus(&this.query.focus_handle(cx), cx);
                    cx.notify();
                }
                AppEvent::Dismiss => this.hide(window, cx),
            });
        self.app = Some((app, subscription));
        cx.notify();
    }

    fn close_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.view = None;
        self.run_task = Task::ready(());
        self.set_query("", PLACEHOLDER, window, cx);
        self.rerank(None, window, cx);
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
        let view_items = self
            .view
            .as_ref()
            .map(|view| view.items.clone())
            .unwrap_or_default();
        let muted = colors.muted;

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
                        Row::Tool(tool) => row
                            .child(div().size(px(24.)).flex().justify_center().child("▦"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(tool.entry.title.clone()),
                            )
                            .child(
                                div()
                                    .max_w(px(300.))
                                    .truncate()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(tool.entry.subtitle.clone()),
                            ),
                        Row::Plugin(command) => row
                            .child(div().size(px(24.)).flex().justify_center().child("›"))
                            .child(div().flex_1().min_w_0().truncate().child(command.title()))
                            .child(
                                div()
                                    .max_w(px(300.))
                                    .truncate()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(command.summary.clone()),
                            ),
                        Row::Item(item) => match view_items.get(*item) {
                            Some(item) => row
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .child(div().truncate().child(item.title.clone()))
                                        .when(!item.subtitle.is_empty(), |text| {
                                            text.child(
                                                div()
                                                    .truncate()
                                                    .text_xs()
                                                    .text_color(muted)
                                                    .child(item.subtitle.clone()),
                                            )
                                        }),
                                )
                                .when(item.url.is_some(), |row| {
                                    row.child(div().text_sm().text_color(muted).child("↗"))
                                }),
                            None => row,
                        },
                        Row::Calc(calc) => row
                            .child(div().size(px(24.)).flex().justify_center().child("="))
                            .child(
                                div()
                                    .flex_1()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(calc.value.clone()),
                            )
                            .child(div().text_sm().text_color(muted).child(calc.detail.clone())),
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

        let footer = match &self.view {
            None => format!(
                "{count} results · ranked in {:.2} ms",
                self.last_rank.as_secs_f64() * 1000.
            ),
            Some(view) => {
                let title = view.command.title();
                let total = view.items.len();
                match &view.state {
                    ViewState::Loading if total == 0 => format!("Running {title}…"),
                    ViewState::Loading => format!("{count} of {total} · {title} · refreshing…"),
                    ViewState::Ready => format!("{count} of {total} · {title} · Esc to go back"),
                    ViewState::Failed(error) => format!("{title} failed: {error}"),
                }
            }
        };

        let radius = px(PANEL_RADIUS);
        let layer = || div().absolute().inset_0().rounded(radius);
        let content = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .text_color(colors.text)
            .child(
                div()
                    .p_3()
                    .border_b_1()
                    .border_color(colors.divider)
                    .text_lg()
                    .child(Input::new(&self.query).appearance(false)),
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
                    .truncate()
                    .child(footer),
            );

        // Glass, bottom to top: blurred backdrop, tint, a sheen fading down from
        // the top edge, then content. gpui-kit's window frame draws the rim and
        // the drop shadow around it.
        let panel = div()
            .relative()
            .size_full()
            .rounded(radius)
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
            .child(match &self.app {
                Some((app, _)) => div()
                    .relative()
                    .size_full()
                    .child(app.clone())
                    .into_any_element(),
                None => content.into_any_element(),
            });

        div()
            .size_full()
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| {
                this.select(this.selected.saturating_sub(1), cx);
            }))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| {
                let last = this.results.len().saturating_sub(1);
                this.select((this.selected + 1).min(last), cx);
            }))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| {
                let results = this.results.clone();
                match results.get(this.selected) {
                    Some(Row::App(app)) => apps::launch(&this.catalog.apps[*app]),
                    Some(Row::Calc(calc)) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(calc.value.clone()))
                    }
                    Some(Row::Plugin(command)) => {
                        return this.open_view(command.clone(), window, cx);
                    }
                    Some(Row::Tool(tool)) => {
                        return this.open_app(tool.clone(), window, cx);
                    }
                    Some(Row::Item(ix)) => {
                        let Some(item) = this.view.as_ref().and_then(|v| v.items.get(*ix)) else {
                            return;
                        };
                        match &item.url {
                            Some(url) => apps::open_url(url),
                            None => {
                                cx.write_to_clipboard(ClipboardItem::new_string(item.title.clone()))
                            }
                        }
                    }
                    None => return,
                }
                this.hide(window, cx);
            }))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| {
                if this.view.is_some() {
                    this.close_view(window, cx);
                } else {
                    this.hide(window, cx);
                }
            }))
            .child(panel)
    }
}

/// A Super-based shortcut makes the window manager grab the keyboard, which
/// hides the open launcher by focus loss just before the toggle arrives;
/// that toggle meant "hide", so it must not re-show the window.
fn hidden_by_same_press(blurred_at: Option<Instant>, requested_at: Instant) -> bool {
    const SAME_PRESS: Duration = Duration::from_millis(400);
    blurred_at.is_some_and(|blurred| {
        requested_at.saturating_duration_since(blurred) < SAME_PRESS
            && blurred.saturating_duration_since(requested_at) < SAME_PRESS
    })
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
    use super::{calculate, hidden_by_same_press};
    use crate::settings::{Settings, Theme};
    use asyar_calculator::locale::NumberFormat;
    use std::time::{Duration, Instant};

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

    #[test]
    fn a_toggle_right_after_a_focus_loss_hide_stays_hidden() {
        let now = Instant::now();
        assert!(hidden_by_same_press(
            Some(now),
            now + Duration::from_millis(50)
        ));
        assert!(hidden_by_same_press(
            Some(now + Duration::from_millis(50)),
            now
        ));
        assert!(!hidden_by_same_press(
            Some(now),
            now + Duration::from_secs(2)
        ));
        assert!(!hidden_by_same_press(None, now));
    }
}

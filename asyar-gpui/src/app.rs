//! A tool's `asyar-ui` app inside the launcher (docs/ui-protocol.md): tabs, a
//! list with a detail pane, an action panel, pickers and text inputs, and
//! toasts. The tool does every read and write; this only renders and routes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::channel::oneshot;
use gpui::component::input::{Input, InputEvent, InputState};
use gpui::component::text::TextView;
use gpui::{
    ClipboardItem, Context, Entity, EventEmitter, Focusable, FontWeight, Hsla, ImageSource,
    KeyDownEvent, ScrollStrategy, SharedString, Subscription, Task, UniformListScrollHandle,
    Window, actions, div, hsla, prelude::*, px, uniform_list,
};

use crate::settings::Glass;
use crate::ui::{self, Accessory, Action, Choice, Data, Item, Params, Tab, View};

pub const CONTEXT: &str = "ToolApp";
const ROW_HEIGHT: f32 = 48.;
const LOCAL_DEBOUNCE: Duration = Duration::from_millis(120);
const REMOTE_DEBOUNCE: Duration = Duration::from_millis(150);
const TOAST_FOR: Duration = Duration::from_secs(3);

actions!(
    tool_app,
    [Up, Down, Confirm, Back, ShowActions, NextTab, PrevTab]
);

pub enum AppEvent {
    /// Esc on an empty search: back to the launcher's root search.
    Close,
    /// An action left the launcher (opened a link): hide the window.
    Dismiss,
}

enum Overlay {
    None,
    Actions {
        actions: Vec<Action>,
        selected: usize,
    },
    Pick {
        action: Action,
        options: Vec<Choice>,
        selected: usize,
    },
    Input {
        action: Action,
        field: Entity<InputState>,
    },
}

pub struct ToolApp {
    program: PathBuf,
    title: String,
    dark: bool,
    frosted: bool,
    query: Entity<InputState>,
    route: String,
    params: Params,
    view: Option<View>,
    /// The view's items, flattened; actions patch them in place.
    items: Rc<Vec<Item>>,
    shown: Rc<Vec<usize>>,
    selected: usize,
    scroll: UniformListScrollHandle,
    loading: bool,
    error: Option<String>,
    tabs: Vec<Tab>,
    badges: HashMap<String, String>,
    /// Last view per route, so tabs switch instantly while they refresh.
    screens: HashMap<String, View>,
    details: HashMap<String, View>,
    detail_key: Option<String>,
    options: HashMap<String, Vec<Choice>>,
    overlay: Overlay,
    toast: Option<(String, bool)>,
    load_task: Task<()>,
    detail_task: Task<()>,
    query_task: Task<()>,
    toast_task: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AppEvent> for ToolApp {}

impl ToolApp {
    pub fn new(
        program: PathBuf,
        title: String,
        route: String,
        (dark, frosted): (bool, bool),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx));
        let subscriptions =
            vec![
                cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.on_query(window, cx);
                    }
                }),
            ];
        window.focus(&query.focus_handle(cx), cx);
        let mut app = Self {
            program,
            title,
            dark,
            frosted,
            query,
            route: String::new(),
            params: Params::new(),
            view: None,
            items: Rc::default(),
            shown: Rc::default(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            loading: false,
            error: None,
            tabs: Vec::new(),
            badges: HashMap::new(),
            screens: HashMap::new(),
            details: HashMap::new(),
            detail_key: None,
            options: HashMap::new(),
            overlay: Overlay::None,
            toast: None,
            load_task: Task::ready(()),
            detail_task: Task::ready(()),
            query_task: Task::ready(()),
            toast_task: Task::ready(()),
            _subscriptions: subscriptions,
        };
        app.open_route(route, Params::new(), window, cx);
        app
    }

    fn query_text(&self, cx: &Context<Self>) -> String {
        self.query.read(cx).value().to_string()
    }

    fn remote(&self) -> bool {
        self.view
            .as_ref()
            .is_some_and(|v| v.search.filtering == "remote")
    }

    /// Shows `route` (cached copy first) and loads it fresh.
    fn open_route(
        &mut self,
        route: String,
        params: Params,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = Overlay::None;
        self.route = route.clone();
        self.params = params;
        self.query.update(cx, |q, cx| q.set_value("", window, cx));
        if let Some(view) = self.screens.get(&route).cloned() {
            self.set_view(view, None, window, cx);
        }
        self.reload(None, window, cx);
    }

    /// Re-runs the current route; `keep` re-selects that item id afterwards.
    fn reload(&mut self, keep: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.loading = true;
        let query = if self.remote() {
            self.query_text(cx)
        } else {
            String::new()
        };
        let rx = run_route(&self.program, &self.route, &self.params, &query);
        let route = self.route.clone();
        self.load_task = cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = rx.await else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                if this.route != route {
                    return;
                }
                this.loading = false;
                match result.map(|data| data.view) {
                    Ok(Some(view)) => {
                        this.error = None;
                        this.screens.insert(route, view.clone());
                        this.set_view(view, keep, window, cx);
                    }
                    Ok(None) => this.error = Some("the tool sent no screen".into()),
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        });
        cx.notify();
    }

    fn set_view(
        &mut self,
        view: View,
        keep: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !view.tabs.is_empty() && self.tabs.is_empty() {
            self.tabs = view.tabs.clone();
            self.load_badges(window, cx);
        }
        let placeholder = match view.search.placeholder.as_str() {
            "" => format!("Search {}…", self.title),
            text => text.to_owned(),
        };
        self.query
            .update(cx, |q, cx| q.set_placeholder(placeholder, window, cx));
        self.items = Rc::new(view.items().cloned().collect());
        self.view = Some(view);
        self.filter(cx);
        let ix = keep
            .and_then(|id| {
                self.shown
                    .iter()
                    .position(|&i| self.items.get(i).is_some_and(|it| it.id == id))
            })
            .unwrap_or(0);
        self.select(ix, window, cx);
    }

    fn load_badges(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for tab in self.tabs.clone() {
            let Some(route) = tab.badge_route else {
                continue;
            };
            let rx = run_route(&self.program, &route, &Params::new(), "");
            cx.spawn_in(window, async move |this, cx| {
                if let Ok(Ok(Data {
                    result: Some(result),
                    ..
                })) = rx.await
                {
                    let _ = this.update(cx, |this, cx| {
                        this.badges
                            .insert(tab.route, result.badge.unwrap_or_default());
                        cx.notify();
                    });
                }
            })
            .detach();
        }
    }

    fn on_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remote() {
            self.query_task = cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(REMOTE_DEBOUNCE).await;
                let _ = this.update_in(cx, |this, window, cx| this.reload(None, window, cx));
            });
            return;
        }
        self.filter(cx);
        self.select(0, window, cx);
    }

    /// Local filtering: every query word in the title, subtitle, keywords or tags.
    fn filter(&mut self, cx: &Context<Self>) {
        let query = if self.remote() {
            String::new()
        } else {
            self.query_text(cx)
        };
        self.shown = Rc::new(filter_items(&query, &self.items));
    }

    fn current(&self) -> Option<(usize, &Item)> {
        let ix = *self.shown.get(self.selected)?;
        Some((ix, self.items.get(ix)?))
    }

    fn select(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = ix.min(self.shown.len().saturating_sub(1));
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        self.load_detail(window, cx);
        cx.notify();
    }

    /// Loads the selected item's detail once the selection rests.
    fn load_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let list_detail = self.view.as_ref().and_then(|v| v.detail.clone());
        let Some((route, params)) = self
            .current()
            .and_then(|(_, item)| item.detail_request(list_detail.as_ref()))
        else {
            self.detail_key = None;
            self.detail_task = Task::ready(());
            return;
        };
        let key = format!("{route} {}", serde_json::Value::Object(params.clone()));
        self.detail_key = Some(key.clone());
        if self.details.contains_key(&key) {
            self.detail_task = Task::ready(());
            return;
        }
        let debounce = Duration::from_millis(
            list_detail
                .and_then(|d| d.debounce_ms)
                .unwrap_or(LOCAL_DEBOUNCE.as_millis() as u64),
        );
        let program = self.program.clone();
        self.detail_task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(debounce).await;
            let Ok(result) = run_route(&program, &route, &params, "").await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Ok(Data {
                    view: Some(view), ..
                }) = result
                {
                    this.details.insert(key, view);
                    cx.notify();
                }
            });
        });
    }

    fn item_actions(&self) -> Vec<Action> {
        let mut actions = self
            .current()
            .map(|(_, item)| item.actions.clone())
            .unwrap_or_default();
        if let Some(view) = &self.view {
            actions.extend(view.actions.iter().cloned());
        }
        actions
    }

    fn trigger(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        match action.kind.as_str() {
            "open" => match action.url.as_deref().and_then(ui::safe_url) {
                Some(url) => {
                    crate::apps::open_url(url);
                    cx.emit(AppEvent::Dismiss);
                }
                None => self.show_toast("That link can't be opened".into(), true, window, cx),
            },
            "copy" => {
                let text = action.text.clone().unwrap_or_default();
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.show_toast("Copied".into(), false, window, cx);
            }
            "run" => self.run_action(action, None, None, window, cx),
            "pick" => self.open_picker(action, window, cx),
            "input" => {
                let placeholder = action
                    .input
                    .as_ref()
                    .map(|i| i.placeholder.clone())
                    .unwrap_or_default();
                let field = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
                window.focus(&field.focus_handle(cx), cx);
                self.overlay = Overlay::Input { action, field };
            }
            kind => self.show_toast(format!("Unsupported action: {kind}"), true, window, cx),
        }
        cx.notify();
    }

    fn open_picker(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let pick = action.pick.clone().unwrap_or_default();
        let cached = pick
            .options_route
            .as_ref()
            .and_then(|r| self.options.get(r).cloned());
        let options = if pick.options.is_empty() {
            cached.unwrap_or_default()
        } else {
            pick.options
        };
        if options.is_empty()
            && let Some(route) = pick.options_route
        {
            let rx = run_route(&self.program, &route, &Params::new(), "");
            let action = action.clone();
            cx.spawn_in(window, async move |this, cx| {
                let result = rx.await;
                let _ = this.update_in(cx, |this, window, cx| match result {
                    Ok(Ok(Data {
                        result: Some(outcome),
                        ..
                    })) if !outcome.options.is_empty() => {
                        this.options.insert(route, outcome.options);
                        this.open_picker(action, window, cx);
                    }
                    Ok(Err(error)) => this.show_toast(error, true, window, cx),
                    _ => this.show_toast("No choices available".into(), true, window, cx),
                });
            })
            .detach();
            self.show_toast("Loading choices…".into(), false, window, cx);
            return;
        }
        self.overlay = Overlay::Pick {
            action,
            options,
            selected: 0,
        };
        cx.notify();
    }

    /// Runs an action route; `label` fills the optimistic patch.
    fn run_action(
        &mut self,
        action: Action,
        value: Option<String>,
        label: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action.safety.as_deref() == Some("destructive") {
            // shortcut: no confirmation dialog yet, so destructive actions are refused.
            self.show_toast(
                "Destructive actions aren't supported yet".into(),
                true,
                window,
                cx,
            );
            return;
        }
        let Some((route, params)) = action.run_with(value.as_deref()) else {
            return;
        };
        let target = self.current().map(|(ix, item)| (ix, item.id.clone()));
        let before = target
            .as_ref()
            .map(|(ix, _)| self.items[*ix].accessories.clone());
        if let (Some(patch), Some((ix, _))) = (&action.optimistic, &target) {
            let label = label.unwrap_or_default();
            Rc::make_mut(&mut self.items)[*ix].accessories = patch
                .accessories
                .iter()
                .cloned()
                .map(|mut a| {
                    a.tag = a.tag.map(|t| t.replace("{label}", &label));
                    a
                })
                .collect();
        }
        self.show_toast(format!("{}…", action.title), false, window, cx);
        let rx = run_route(&self.program, &route, &params, "");
        cx.spawn_in(window, async move |this, cx| {
            let result = rx.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let restore = |this: &mut Self| {
                    if let (Some((ix, _)), Some(before)) = (&target, before.clone())
                        && let Some(item) = Rc::make_mut(&mut this.items).get_mut(*ix)
                    {
                        item.accessories = before;
                    }
                };
                match result {
                    Ok(Ok(data)) => {
                        let outcome = data.result.unwrap_or_default();
                        if let (Some(patch), Some((ix, _))) = (outcome.patch, &target)
                            && let Some(item) = Rc::make_mut(&mut this.items).get_mut(*ix)
                        {
                            item.accessories = patch.accessories;
                        }
                        let toast = outcome.toast.unwrap_or_else(|| "Done".into());
                        this.show_toast(toast, false, window, cx);
                        if outcome.refresh {
                            this.details.clear();
                            this.reload(target.map(|(_, id)| id), window, cx);
                        }
                    }
                    Ok(Err(error)) => {
                        restore(this);
                        this.show_toast(error, true, window, cx);
                    }
                    Err(_) => restore(this),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn show_toast(
        &mut self,
        text: String,
        error: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toast = Some((text, error));
        self.toast_task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(TOAST_FOR).await;
            let _ = this.update(cx, |this, cx| {
                this.toast = None;
                cx.notify();
            });
        });
        cx.notify();
    }

    fn switch_tab(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.tabs.len() as isize;
        if count == 0 {
            return;
        }
        let at = self
            .tabs
            .iter()
            .position(|t| t.route == self.route)
            .unwrap_or(0) as isize;
        let next = self.tabs[(at + step).rem_euclid(count) as usize]
            .route
            .clone();
        self.open_route(next, Params::new(), window, cx);
    }

    fn move_by(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let len = match &self.overlay {
            Overlay::Actions { actions, .. } => actions.len(),
            Overlay::Pick { options, .. } => options.len(),
            Overlay::Input { .. } => return,
            Overlay::None => {
                let next = (self.selected as isize + step).max(0) as usize;
                return self.select(next, window, cx);
            }
        };
        if let Overlay::Actions { selected, .. } | Overlay::Pick { selected, .. } =
            &mut self.overlay
        {
            *selected = ((*selected as isize + step).max(0) as usize).min(len.saturating_sub(1));
        }
        cx.notify();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match std::mem::replace(&mut self.overlay, Overlay::None) {
            Overlay::Actions { actions, selected } => {
                if let Some(action) = actions.get(selected).cloned() {
                    self.trigger(action, window, cx);
                }
            }
            Overlay::Pick {
                action,
                options,
                selected,
            } => {
                if let Some(choice) = options.get(selected) {
                    let (value, label) = (choice.value.clone(), choice.title.clone());
                    self.run_action(action, Some(value), Some(label), window, cx);
                }
            }
            Overlay::Input { action, field } => {
                let text = field.read(cx).value().trim().to_string();
                if text.is_empty() {
                    self.overlay = Overlay::Input { action, field };
                    return self.show_toast("Type something first".into(), true, window, cx);
                }
                window.focus(&self.query.focus_handle(cx), cx);
                self.run_action(action, Some(text), None, window, cx);
            }
            Overlay::None => {
                // Enter runs the item's first action (by convention, open).
                if let Some(action) = self.item_actions().into_iter().next() {
                    self.trigger(action, window, cx);
                }
            }
        }
        cx.notify();
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.overlay, Overlay::None) {
            self.overlay = Overlay::None;
            window.focus(&self.query.focus_handle(cx), cx);
        } else if !self.query_text(cx).is_empty() {
            self.query.update(cx, |q, cx| q.set_value("", window, cx));
        } else {
            cx.emit(AppEvent::Close);
        }
        cx.notify();
    }

    /// Alt+letter runs the item action whose `shortcut` is that letter.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keys = &event.keystroke;
        if !keys.modifiers.alt || keys.modifiers.control || keys.key.chars().count() != 1 {
            return;
        }
        if let Some(action) = self
            .item_actions()
            .into_iter()
            .find(|a| a.shortcut.as_deref() == Some(keys.key.as_str()))
        {
            cx.stop_propagation();
            self.trigger(action, window, cx);
        }
    }
}

impl Render for ToolApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Glass::new(self.dark, self.frosted);
        let dark = self.dark;
        let muted = colors.muted;

        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .p_3()
            .border_b_1()
            .border_color(colors.divider)
            .text_lg()
            .child(
                div()
                    .px_2()
                    .rounded_md()
                    .bg(colors.selection)
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .child(Input::new(&self.query).appearance(false)),
            );

        let tabs = (!self.tabs.is_empty()).then(|| {
            let filters = self
                .view
                .as_ref()
                .map(|v| v.filters.clone())
                .unwrap_or_default();
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_3()
                .py_1()
                .border_b_1()
                .border_color(colors.divider)
                .text_sm()
                .children(self.tabs.iter().enumerate().map(|(i, tab)| {
                    let active = tab.route == self.route;
                    let badge = self.badges.get(&tab.route).filter(|b| !b.is_empty());
                    div()
                        .id(("tab", i))
                        .px_2()
                        .py(px(2.))
                        .rounded_md()
                        .when(active, |d| {
                            d.bg(colors.selection).font_weight(FontWeight::MEDIUM)
                        })
                        .when(!active, |d| d.text_color(muted))
                        .flex()
                        .gap_1()
                        .child(tab.title.clone())
                        .children(badge.map(|b| tag(b, Some("danger"), dark)))
                        .on_click({
                            let route = tab.route.clone();
                            cx.listener(move |this, _, window, cx| {
                                this.open_route(route.clone(), Params::new(), window, cx)
                            })
                        })
                }))
                .child(div().flex_1())
                .children(filters.into_iter().enumerate().map(|(i, filter)| {
                    let current = filter
                        .options
                        .iter()
                        .find(|o| o.value == filter.value)
                        .map(|o| o.title.clone())
                        .unwrap_or(filter.value.clone());
                    div()
                        .id(("filter", i))
                        .px_2()
                        .rounded_md()
                        .text_color(muted)
                        .child(format!("{}: {current} ▾", filter.title))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            // Cycles to the next option and reloads.
                            let at = filter
                                .options
                                .iter()
                                .position(|o| o.value == filter.value)
                                .map_or(0, |p| (p + 1) % filter.options.len().max(1));
                            if let Some(next) = filter.options.get(at) {
                                this.params.insert(
                                    filter.id.clone(),
                                    serde_json::Value::String(next.value.clone()),
                                );
                                this.reload(None, window, cx);
                            }
                        }))
                }))
        });

        let items = self.items.clone();
        let shown = self.shown.clone();
        let selected = self.selected;
        let selection = colors.selection;
        let list = uniform_list("tool-items", shown.len(), move |range, _, _| {
            range
                .filter_map(|row| {
                    let item = items.get(*shown.get(row)?)?;
                    Some(
                        div()
                            .id(row)
                            .h(px(ROW_HEIGHT))
                            .px_3()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded_lg()
                            .when(row == selected, |d| d.bg(selection))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(div().truncate().child(item.title.clone()))
                                    .when(!item.subtitle.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .truncate()
                                                .text_xs()
                                                .text_color(muted)
                                                .child(item.subtitle.clone()),
                                        )
                                    }),
                            )
                            .children(
                                item.accessories
                                    .iter()
                                    .take(2)
                                    .map(|a| accessory(a, muted, dark)),
                            ),
                    )
                })
                .collect()
        })
        .track_scroll(&self.scroll)
        .size_full()
        .px_1();

        let detail = self.render_detail(&colors, cx);
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(
                div()
                    .w(px(380.))
                    .h_full()
                    .py_1()
                    .border_r_1()
                    .border_color(colors.divider)
                    .child(list),
            )
            .child(detail);

        let overlay = self.render_overlay(&colors);

        let status: SharedString = match (&self.toast, &self.error) {
            (Some((text, _)), _) => text.clone().into(),
            (None, Some(error)) => format!("{} failed: {error}", self.title).into(),
            (None, None) if self.loading && self.items.is_empty() => "Loading…".into(),
            (None, None) => format!(
                "{} of {}{}",
                self.shown.len(),
                self.items.len(),
                if self.loading {
                    " · refreshing…"
                } else {
                    ""
                }
            )
            .into(),
        };
        let failed = self.toast.as_ref().is_some_and(|t| t.1)
            || (self.toast.is_none() && self.error.is_some());
        let hints = self.item_actions().into_iter().take(3).map(|a| {
            let key = match a.shortcut.as_deref() {
                Some("enter") => "⏎".to_owned(),
                Some(k) => format!("Alt+{}", k.to_uppercase()),
                None => String::new(),
            };
            div()
                .flex()
                .gap_1()
                .child(div().font_weight(FontWeight::MEDIUM).child(key))
                .child(a.title)
        });
        let footer = div()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_2()
            .border_t_1()
            .border_color(colors.divider)
            .text_xs()
            .text_color(muted)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(failed, |d| d.text_color(tone_fg(Some("danger"), dark)))
                    .child(status),
            )
            .children(hints)
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(div().font_weight(FontWeight::MEDIUM).child("Ctrl+K"))
                    .child("Actions"),
            );

        div()
            .key_context(CONTEXT)
            .size_full()
            .flex()
            .flex_col()
            .text_color(colors.text)
            .capture_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(|this, _: &Up, window, cx| this.move_by(-1, window, cx)))
            .on_action(cx.listener(|this, _: &Down, window, cx| this.move_by(1, window, cx)))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &Back, window, cx| this.back(window, cx)))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.switch_tab(1, window, cx)))
            .on_action(cx.listener(|this, _: &PrevTab, window, cx| this.switch_tab(-1, window, cx)))
            .on_action(cx.listener(|this, _: &ShowActions, _, cx| {
                this.overlay = Overlay::Actions {
                    actions: this.item_actions(),
                    selected: 0,
                };
                cx.notify();
            }))
            .child(header)
            .children(tabs)
            .child(body)
            .children(overlay)
            .child(footer)
    }
}

impl ToolApp {
    fn render_detail(&self, colors: &Glass, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let muted = colors.muted;
        let pane = div()
            .id("tool-detail")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_2();
        let loaded = self.detail_key.as_ref().and_then(|k| self.details.get(k));
        // Until the detail arrives, the item's own fields fill the pane.
        let (title, tags, fields, markdown, sections) = match (loaded, self.current()) {
            (Some(v), _) => (
                v.title.clone(),
                v.tags.clone(),
                v.fields.clone(),
                v.markdown.clone(),
                v.sections.clone(),
            ),
            (None, Some((_, item))) => (
                item.title.clone(),
                item.accessories
                    .iter()
                    .filter(|a| a.tag.is_some())
                    .cloned()
                    .collect(),
                item.fields.clone(),
                String::new(),
                Vec::new(),
            ),
            (None, None) => {
                let empty = if self.loading { "" } else { "Nothing here" };
                return pane.child(div().text_color(muted).child(empty));
            }
        };
        let field_rows = fields.into_iter().map(|f| {
            let value = match (f.value, f.progress, f.link) {
                (_, Some(p), _) => div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(120.))
                            .h(px(6.))
                            .rounded_full()
                            .bg(colors.divider)
                            .child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .w(px(120. * p.clamp(0., 1.)))
                                    .bg(tone_fg(Some("success"), dark)),
                            ),
                    )
                    .child(format!("{}%", (p * 100.).round()))
                    .into_any_element(),
                (_, _, Some(link)) if ui::safe_url(&link).is_some() => div()
                    .id(SharedString::from(link.clone()))
                    .text_color(tone_fg(Some("info"), dark))
                    .truncate()
                    .child(link.clone())
                    .on_click(move |_, _, _| crate::apps::open_url(&link))
                    .into_any_element(),
                (Some(v), _, _) => div().truncate().child(v).into_any_element(),
                _ => div().into_any_element(),
            };
            div()
                .flex()
                .gap_3()
                .text_sm()
                .child(
                    div()
                        .w(px(90.))
                        .flex_none()
                        .text_color(muted)
                        .child(f.label),
                )
                .child(div().flex_1().min_w_0().child(value))
        });
        let sections = sections.into_iter().enumerate().map(|(i, s)| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .pt_2()
                .children(s.title.map(|t| {
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(muted)
                        .child(t)
                }))
                .children(s.timeline.into_iter().enumerate().map(move |(j, e)| {
                    let body = e.markdown.or(e.text).unwrap_or_default();
                    div()
                        .flex()
                        .flex_col()
                        .py_1()
                        .border_b_1()
                        .border_color(colors.divider)
                        .child(div().text_xs().text_color(muted).child(format!(
                            "{} · {}",
                            e.who,
                            relative_date(&e.at)
                        )))
                        .child(markdown_view(("event", i * 10_000 + j), body))
                }))
                .children(s.items.into_iter().map(|it| {
                    div()
                        .text_sm()
                        .truncate()
                        .child(format!("#{} {}", it.id, it.title))
                }))
        });
        let _ = cx;
        pane.child(
            div()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div().flex().flex_wrap().gap_1().children(
                tags.iter()
                    .filter_map(|a| a.tag.as_ref().map(|t| tag(t, a.tone.as_deref(), dark))),
            ),
        )
        .children(field_rows)
        .when(!markdown.is_empty(), |d| {
            d.child(
                div()
                    .pt_2()
                    .text_sm()
                    .child(markdown_view("description", markdown)),
            )
        })
        .children(sections)
    }

    fn render_overlay(&self, colors: &Glass) -> Option<impl IntoElement> {
        let panel = || {
            div()
                .mx_3()
                .mb_2()
                .p_2()
                .rounded_lg()
                .border_1()
                .border_color(colors.divider)
                .bg(colors.tint)
                .flex()
                .flex_col()
                .gap(px(2.))
                .text_sm()
        };
        let row = |text: SharedString, on: bool, colors: &Glass| {
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .when(on, |d| d.bg(colors.selection))
                .child(text)
        };
        match &self.overlay {
            Overlay::None => None,
            Overlay::Actions { actions, selected } => Some(
                panel()
                    .child(div().text_xs().text_color(colors.muted).child("Actions"))
                    .children(actions.iter().enumerate().map(|(i, a)| {
                        let key = match a.shortcut.as_deref() {
                            Some("enter") => "⏎".to_owned(),
                            Some(k) => format!("Alt+{}", k.to_uppercase()),
                            None => String::new(),
                        };
                        row(
                            format!("{}   {key}", a.title).into(),
                            i == *selected,
                            colors,
                        )
                    }))
                    .into_any_element(),
            ),
            Overlay::Pick {
                action,
                options,
                selected,
            } => Some(
                panel()
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted)
                            .child(action.title.clone()),
                    )
                    .children(options.iter().enumerate().map(|(i, o)| {
                        row("".into(), i == *selected, colors).child(tag(
                            &o.title,
                            o.tone.as_deref(),
                            self.dark,
                        ))
                    }))
                    .into_any_element(),
            ),
            Overlay::Input { action, field } => Some(
                panel()
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted)
                            .child(format!("{} · Enter to send, Esc to cancel", action.title)),
                    )
                    .child(Input::new(field))
                    .into_any_element(),
            ),
        }
    }
}

/// Markdown from a tool: rendered natively, never fetching remote images.
fn markdown_view(id: impl Into<gpui::ElementId>, text: String) -> impl IntoElement {
    TextView::markdown(id, text).image_source(|_| ImageSource::from(PathBuf::new()))
}

fn accessory(a: &Accessory, muted: Hsla, dark: bool) -> gpui::AnyElement {
    if let Some(t) = &a.tag {
        return tag(t, a.tone.as_deref(), dark).into_any_element();
    }
    let text = a
        .date
        .as_deref()
        .map(relative_date)
        .or_else(|| a.text.clone())
        .unwrap_or_default();
    div()
        .text_xs()
        .text_color(muted)
        .flex_none()
        .child(text)
        .into_any_element()
}

fn tag(text: &str, tone: Option<&str>, dark: bool) -> impl IntoElement {
    div()
        .px(px(6.))
        .rounded_md()
        .text_xs()
        .flex_none()
        .bg(tone_bg(tone, dark))
        .text_color(tone_fg(tone, dark))
        .child(text.to_owned())
}

fn tone_hue(tone: Option<&str>) -> Option<f32> {
    match tone? {
        "info" => Some(0.6),
        "success" => Some(0.38),
        "warning" => Some(0.11),
        "danger" => Some(0.0),
        _ => None,
    }
}

fn tone_bg(tone: Option<&str>, dark: bool) -> Hsla {
    match tone_hue(tone) {
        Some(h) => hsla(h, 0.65, if dark { 0.3 } else { 0.9 }, 0.55),
        None => hsla(0., 0., if dark { 1. } else { 0. }, 0.08),
    }
}

fn tone_fg(tone: Option<&str>, dark: bool) -> Hsla {
    match tone_hue(tone) {
        Some(h) => hsla(h, 0.7, if dark { 0.75 } else { 0.32 }, 1.),
        None => hsla(0., 0., if dark { 0.85 } else { 0.25 }, 1.),
    }
}

/// Every query word in an item's title, subtitle, keywords or tags.
fn filter_items(query: &str, items: &[Item]) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let mut text = format!(
                "{} {} {}",
                item.title,
                item.subtitle,
                item.keywords.join(" ")
            );
            for a in &item.accessories {
                text.push(' ');
                text.push_str(a.tag.as_deref().unwrap_or_default());
            }
            let text = text.to_lowercase();
            words.iter().all(|w| text.contains(w.as_str()))
        })
        .map(|(ix, _)| ix)
        .collect()
}

/// `2026-11-24T10:16:52Z` → "4m", "6h", "3d", or "24 Nov" past a week.
fn relative_date(iso: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    relative_to(iso, now)
}

fn relative_to(iso: &str, now: i64) -> String {
    let Some(then) = parse_iso(iso) else {
        return iso.to_owned();
    };
    let ago = (now - then).max(0);
    match ago {
        0..60 => "now".into(),
        60..3_600 => format!("{}m", ago / 60),
        3_600..86_400 => format!("{}h", ago / 3_600),
        86_400..604_800 => format!("{}d", ago / 86_400),
        _ => {
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            let (month, day) = (iso[5..7].parse::<usize>().unwrap_or(1), &iso[8..10]);
            format!(
                "{} {}",
                day.trim_start_matches('0'),
                MONTHS[(month - 1).min(11)]
            )
        }
    }
}

/// Seconds since the epoch for `YYYY-MM-DDTHH:MM:SS…` in UTC (a trailing
/// zone other than Z is ignored).
fn parse_iso(iso: &str) -> Option<i64> {
    let num = |r: std::ops::Range<usize>| iso.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    // Days from civil (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3_600 + mm * 60 + ss)
}

/// Runs a route on a background thread.
fn run_route(
    program: &std::path::Path,
    route: &str,
    params: &Params,
    query: &str,
) -> oneshot::Receiver<Result<Data, String>> {
    let (tx, rx) = oneshot::channel();
    let (program, route, params, query) = (
        program.to_owned(),
        route.to_owned(),
        params.clone(),
        query.to_owned(),
    );
    std::thread::spawn(move || {
        let _ = tx.send(ui::run(&program, &route, &params, &query));
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_as_relative_ages() {
        let now = parse_iso("2026-11-24T12:00:00Z").unwrap();
        assert_eq!(relative_to("2026-11-24T11:56:00Z", now), "4m");
        assert_eq!(relative_to("2026-11-24T06:00:00Z", now), "6h");
        assert_eq!(relative_to("2026-11-21T12:00:00Z", now), "3d");
        assert_eq!(relative_to("2026-10-05T12:00:00Z", now), "5 Oct");
        assert_eq!(relative_to("not a date", now), "not a date");
        // 2000-03-01 is 11017 days after the epoch.
        assert_eq!(parse_iso("2000-03-01T00:00:00Z"), Some(11_017 * 86_400));
    }

    #[test]
    fn local_filter_matches_words_across_title_keywords_and_tags() {
        let item = |id: &str, title: &str, tag: &str| Item {
            id: id.into(),
            title: title.into(),
            keywords: vec!["SBCOLLEGE".into()],
            accessories: vec![Accessory {
                tag: Some(tag.into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let items = vec![
            item("1", "Fix login", "New"),
            item("2", "Exam grades", "Resolved"),
        ];
        assert_eq!(filter_items("resolved", &items), [1]);
        assert_eq!(filter_items("login sbcollege", &items), [0]);
        assert_eq!(filter_items("", &items), [0, 1]);
    }
}

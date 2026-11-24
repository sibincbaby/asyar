//! `asyar-ui/1` (see docs/ui-protocol.md): a tool describes screens and
//! actions as JSON from `<program> ui <route>`; the launcher renders them with
//! its own components. Every field is optional so older and newer tools both
//! parse; unknown fields are ignored.

use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value, json};

pub const SCHEMA: &str = "asyar-ui/1";
/// Replies relay remote data: cap what the launcher keeps.
const MAX_ITEMS: usize = 5_000;
const MAX_MARKDOWN: usize = 200_000;
const MAX_TEXT: usize = 500;

pub type Params = Map<String, Value>;

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Data {
    pub manifest: Option<Manifest>,
    pub view: Option<View>,
    pub result: Option<Outcome>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub id: String,
    pub title: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub route: String,
    pub title: String,
    pub subtitle: String,
    pub keywords: Vec<String>,
}

/// A `list` or `detail` screen; fields of the other kind stay empty.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct View {
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub tabs: Vec<Tab>,
    pub search: Search,
    pub filters: Vec<Filter>,
    pub detail: Option<DetailRef>,
    pub sections: Vec<Section>,
    pub actions: Vec<Action>,
    pub tags: Vec<Accessory>,
    pub fields: Vec<Field>,
    pub markdown: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Tab {
    pub route: String,
    pub title: String,
    pub badge_route: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Search {
    pub placeholder: String,
    /// `local` (default) or `remote`.
    pub filtering: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Filter {
    pub id: String,
    pub title: String,
    pub value: String,
    pub options: Vec<Choice>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct DetailRef {
    pub route: String,
    /// The list-level form: `params[param] = item.id`.
    pub param: Option<String>,
    pub params: Params,
    pub debounce_ms: Option<u64>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Section {
    pub title: Option<String>,
    pub items: Vec<Item>,
    pub timeline: Vec<Event>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub accessories: Vec<Accessory>,
    pub keywords: Vec<String>,
    pub fields: Vec<Field>,
    pub actions: Vec<Action>,
    pub detail: Option<DetailRef>,
    /// Sub-item rows (e.g. subtasks) that open this route with their id.
    pub route: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct Accessory {
    pub text: Option<String>,
    pub tag: Option<String>,
    pub tone: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Field {
    pub label: String,
    pub value: Option<String>,
    pub progress: Option<f32>,
    pub link: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Event {
    pub at: String,
    pub who: String,
    pub text: Option<String>,
    pub markdown: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Action {
    pub id: String,
    pub title: String,
    pub shortcut: Option<String>,
    pub safety: Option<String>,
    pub kind: String,
    pub url: Option<String>,
    pub text: Option<String>,
    pub pick: Option<Pick>,
    pub input: Option<InputSpec>,
    pub run: Option<Run>,
    pub optimistic: Option<Patch>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Pick {
    pub options: Vec<Choice>,
    pub options_route: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Choice {
    pub value: String,
    pub title: String,
    pub tone: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct InputSpec {
    pub placeholder: String,
    pub multiline: bool,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Run {
    pub route: String,
    pub params: Params,
    pub input_param: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Patch {
    pub accessories: Vec<Accessory>,
}

/// An action route's reply.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Outcome {
    pub toast: Option<String>,
    pub refresh: bool,
    pub patch: Option<Patch>,
    pub options: Vec<Choice>,
    pub badge: Option<String>,
}

impl View {
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.sections.iter().flat_map(|s| s.items.iter())
    }
}

impl Item {
    /// The screen the detail pane shows for this item, if any.
    pub fn detail_request(&self, list: Option<&DetailRef>) -> Option<(String, Params)> {
        if let Some(own) = &self.detail {
            return Some((own.route.clone(), own.params.clone()));
        }
        let list = list?;
        let mut params = list.params.clone();
        params.insert(
            list.param.clone().unwrap_or_else(|| "id".into()),
            Value::String(self.id.clone()),
        );
        Some((list.route.clone(), params))
    }
}

impl Action {
    /// The route and params to run once a pick or input supplies `value`.
    pub fn run_with(&self, value: Option<&str>) -> Option<(String, Params)> {
        let run = self.run.as_ref()?;
        let mut params = run.params.clone();
        if let (Some(key), Some(value)) = (&run.input_param, value) {
            params.insert(key.clone(), Value::String(value.to_owned()));
        }
        Some((run.route.clone(), params))
    }
}

/// Only `http(s)` links open: the data comes from a remote server.
pub fn safe_url(url: &str) -> Option<&str> {
    (url.starts_with("https://") || url.starts_with("http://")).then_some(url)
}

/// Runs `<program> ui <route>` with the request on stdin. Blocks; call it off
/// the UI thread.
pub fn run(program: &Path, route: &str, params: &Params, query: &str) -> Result<Data, String> {
    let request = json!({
        "schema": SCHEMA,
        "accepts": [SCHEMA],
        "route": route,
        "params": params,
        "query": query,
        "host": {"name": "asyar-gpui", "version": env!("CARGO_PKG_VERSION")},
    });
    let output = crate::plugins::exec(
        program,
        ["ui", route],
        crate::plugins::RUN_TIMEOUT,
        Some(request.to_string().as_bytes()),
    )?;
    parse(crate::plugins::envelope_data(&output)?)
}

fn parse(data: Value) -> Result<Data, String> {
    let mut data: Data = serde_json::from_value(data).map_err(|e| format!("bad reply: {e}"))?;
    if let Some(view) = &mut data.view {
        cap(view);
    }
    Ok(data)
}

fn cap(view: &mut View) {
    let mut left = MAX_ITEMS;
    for section in &mut view.sections {
        section.items.truncate(left);
        left -= section.items.len();
        for item in &mut section.items {
            clip(&mut item.title, MAX_TEXT);
            clip(&mut item.subtitle, MAX_TEXT);
        }
        section.timeline.truncate(MAX_ITEMS);
    }
    clip(&mut view.title, MAX_TEXT);
    clip(&mut view.markdown, MAX_MARKDOWN);
}

fn clip(text: &mut String, max: usize) {
    if text.len() > max {
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_list_view_and_ignores_unknown_fields() {
        let data = parse(json!({"view": {
            "type": "list", "title": "Tasks", "future_field": 1,
            "tabs": [{"route": "tasks", "title": "Tasks"}],
            "search": {"filtering": "remote"},
            "detail": {"route": "task", "param": "id"},
            "sections": [{"items": [
                {"id": "7", "title": "Fix login", "accessories": [{"tag": "New", "tone": "info"}]},
                {"id": "n1", "title": "Ping", "detail": {"route": "task", "params": {"id": "9"}}}
            ]}]
        }}))
        .unwrap();
        let view = data.view.unwrap();
        assert_eq!(view.search.filtering, "remote");
        let items: Vec<&Item> = view.items().collect();
        let (route, params) = items[0].detail_request(view.detail.as_ref()).unwrap();
        assert_eq!((route.as_str(), &params["id"]), ("task", &json!("7")));
        // An item's own detail wins over the list's.
        let (_, params) = items[1].detail_request(view.detail.as_ref()).unwrap();
        assert_eq!(params["id"], json!("9"));
    }

    #[test]
    fn action_runs_carry_the_picked_or_typed_value() {
        let action: Action = serde_json::from_value(json!({
            "kind": "input",
            "run": {"route": "comment", "params": {"id": "7"}, "input_param": "text"}
        }))
        .unwrap();
        let (route, params) = action.run_with(Some("done")).unwrap();
        assert_eq!(route, "comment");
        assert_eq!(params["id"], json!("7"));
        assert_eq!(params["text"], json!("done"));
    }

    #[test]
    fn caps_untrusted_sizes_and_blocks_unsafe_links() {
        let long = "é".repeat(400);
        let data = parse(json!({"view": {"type": "detail", "title": long}})).unwrap();
        assert!(data.view.unwrap().title.len() <= MAX_TEXT);
        assert_eq!(safe_url("https://x.test/a"), Some("https://x.test/a"));
        assert_eq!(safe_url("javascript:alert(1)"), None);
        assert_eq!(safe_url("file:///etc/passwd"), None);
    }
}

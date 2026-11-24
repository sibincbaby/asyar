//! CLI plugins: each `$XDG_CONFIG_HOME/asyar-gpui/plugins/*.json` manifest
//! names a program that describes itself with `<program> commands --json`
//! (see the README for the contract). Its read-only commands that take no
//! arguments become launcher commands; running one lists its JSON natively.
//!
//! Manifest: `{"name": "tool", "program": "/home/me/.local/bin/mytool"}`.
//! Plugins run as separate processes, so a hung request never blocks the UI,
//! and their sign-in stays inside the plugin.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

const CATALOG_TIMEOUT: Duration = Duration::from_secs(10);
const RUN_TIMEOUT: Duration = Duration::from_secs(30);
const STDOUT_CAP: u64 = 16 << 20;
const STDERR_CAP: u64 = 64 << 10;

/// One runnable plugin command, e.g. `tool inbox`.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginCommand {
    pub plugin: String,
    pub program: PathBuf,
    /// Subcommand words, passed as separate arguments: `["issue", "list"]`.
    pub words: Vec<String>,
    pub summary: String,
}

impl PluginCommand {
    pub fn title(&self) -> String {
        format!("{} {}", self.plugin, self.words.join(" "))
    }

    /// Runs the command and turns its JSON into list rows. Blocks; call it
    /// off the UI thread.
    pub fn run(&self) -> Result<Vec<Item>, String> {
        let args = self.words.iter().map(String::as_str).chain(["--json"]);
        let output = exec(&self.program, args, RUN_TIMEOUT)?;
        parse_output(&output)
    }
}

/// A row of a plugin's result list.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub title: String,
    pub subtitle: String,
    /// Only `http(s)` links: the data comes from a remote server.
    pub url: Option<String>,
}

/// Rows whose title or subtitle contain every word of `query`.
pub fn filter(query: &str, items: &[Item]) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let text = format!("{} {}", item.title, item.subtitle).to_lowercase();
            words.iter().all(|word| text.contains(word.as_str()))
        })
        .map(|(ix, _)| ix)
        .collect()
}

pub fn dir() -> Option<PathBuf> {
    crate::settings::config_dir().map(|dir| dir.join("plugins"))
}

/// Changes whenever a manifest or a plugin program changes, so the launcher
/// re-reads catalogs only then (plugins are rebuilt often during development).
pub type Fingerprint = Vec<(PathBuf, Option<SystemTime>)>;

struct Manifest {
    name: String,
    program: PathBuf,
}

fn manifests(dir: &Path) -> Vec<(PathBuf, Manifest)> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.retain(|p| p.extension().is_some_and(|ext| ext == "json"));
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let manifest = std::fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|v| {
                    let program = PathBuf::from(v.get("program")?.as_str()?);
                    let name = v
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| Some(program.file_name()?.to_string_lossy().into_owned()))?;
                    // Absolute only: a hotkey-started daemon has a minimal PATH.
                    program.is_absolute().then_some(Manifest { name, program })
                });
            if manifest.is_none() {
                eprintln!(
                    "[asyar-gpui] skipping plugin {}: needs an absolute \"program\"",
                    path.display()
                );
            }
            Some((path, manifest?))
        })
        .collect()
}

pub fn fingerprint(dir: &Path) -> Fingerprint {
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut print = vec![(dir.to_owned(), mtime(dir))];
    for (path, manifest) in manifests(dir) {
        print.push((path.clone(), mtime(&path)));
        print.push((manifest.program.clone(), mtime(&manifest.program)));
    }
    print
}

/// Reads every plugin's catalog. Blocks for up to 10 s per plugin.
pub fn discover(dir: &Path) -> Vec<PluginCommand> {
    manifests(dir)
        .into_iter()
        .flat_map(|(_, manifest)| {
            let catalog = exec(&manifest.program, ["commands", "--json"], CATALOG_TIMEOUT)
                .and_then(|out| {
                    serde_json::from_slice::<Value>(&out.stdout).map_err(|e| e.to_string())
                });
            match catalog {
                Ok(catalog) => parse_catalog(&manifest.name, &manifest.program, &catalog),
                Err(error) => {
                    eprintln!("[asyar-gpui] plugin {}: {error}", manifest.name);
                    Vec::new()
                }
            }
        })
        .collect()
}

/// Read-only commands that need no arguments. Anything that writes needs a
/// confirmation flow, which does not exist yet.
fn parse_catalog(plugin: &str, program: &Path, catalog: &Value) -> Vec<PluginCommand> {
    let Some(commands) = catalog.pointer("/data/commands").and_then(Value::as_array) else {
        return Vec::new();
    };
    commands
        .iter()
        .filter(|c| c.get("safety").and_then(Value::as_str) == Some("read"))
        .filter(|c| {
            !c.get("args").and_then(Value::as_array).is_some_and(|args| {
                args.iter()
                    .any(|a| a.get("required") == Some(&Value::Bool(true)))
            })
        })
        .filter_map(|c| {
            let name = c.get("name")?.as_str()?;
            (name != "commands").then(|| PluginCommand {
                plugin: plugin.to_owned(),
                program: program.to_owned(),
                words: name.split_whitespace().map(str::to_owned).collect(),
                summary: c
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect()
}

struct Output {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// No shell: arguments go to the program as-is.
fn exec<'a>(
    program: &Path,
    args: impl IntoIterator<Item = &'a str>,
    timeout: Duration,
) -> Result<Output, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run {}: {e}", program.display()))?;
    // Drained on their own threads so a full pipe can't stall the child.
    let stdout = drain(child.stdout.take(), STDOUT_CAP);
    let stderr = drain(child.stderr.take(), STDERR_CAP);
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(15)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {} s", timeout.as_secs()));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(Output {
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn drain(pipe: Option<impl Read + Send + 'static>, cap: u64) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = pipe {
            let _ = pipe.take(cap).read_to_end(&mut buf);
        }
        buf
    })
}

/// Errors arrive as an `{ok: false, error}` envelope on stdout whatever the
/// exit code; a crash leaves only stderr.
fn parse_output(output: &Output) -> Result<Vec<Item>, String> {
    let Ok(envelope) = serde_json::from_slice::<Value>(&output.stdout) else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let last = stderr.lines().rev().find(|l| !l.trim().is_empty());
        return Err(last.unwrap_or("no JSON output").trim().to_owned());
    };
    if envelope.get("ok") == Some(&Value::Bool(false)) {
        let error = envelope.get("error");
        let field = |k| error.and_then(|e| e.get(k)).and_then(Value::as_str);
        return Err(match (field("message"), field("code")) {
            (Some(message), Some(code)) => format!("{message} ({code})"),
            (Some(message), None) => message.to_owned(),
            _ => "the command failed".to_owned(),
        });
    }
    Ok(items(envelope.get("data").unwrap_or(&Value::Null)))
}

const TITLE_KEYS: [&str; 4] = ["title", "subject", "name", "summary"];
const DETAIL_KEYS: [&str; 7] = [
    "status", "assignee", "project", "client", "actor", "state", "body",
];
const URL_KEYS: [&str; 3] = ["url", "web_url", "html_url"];

/// The first list in `data` becomes rows; an object without one becomes
/// `key  value` rows.
// shortcut: field names are guessed from common keys; once plugins emit a
// shared item shape (title/subtitle/url), read that and drop the guessing.
fn items(data: &Value) -> Vec<Item> {
    let list = data.as_array().or_else(|| {
        let object = data.as_object()?;
        ["items", "results"]
            .iter()
            .find_map(|k| object.get(*k)?.as_array())
            .or_else(|| {
                object.values().find_map(|v| {
                    v.as_array()
                        .filter(|a| !a.is_empty() && a.iter().all(Value::is_object))
                })
            })
    });
    match (list, data) {
        (Some(list), _) => list.iter().map(item).collect(),
        (None, Value::Object(object)) => {
            let mut rows = Vec::new();
            flatten("", object, &mut rows);
            rows
        }
        (None, Value::Null) => Vec::new(),
        (None, other) => vec![item(other)],
    }
}

fn flatten(prefix: &str, object: &serde_json::Map<String, Value>, rows: &mut Vec<Item>) {
    for (key, value) in object {
        let key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Value::Object(inner) if prefix.is_empty() => flatten(&key, inner, rows),
            _ => rows.push(Item {
                title: key,
                subtitle: scalar(value).unwrap_or_else(|| summarize(value)),
                url: None,
            }),
        }
    }
}

fn summarize(value: &Value) -> String {
    match value {
        Value::Array(a) => format!("{} items", a.len()),
        Value::Null => "—".to_owned(),
        other => other.to_string(),
    }
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn item(value: &Value) -> Item {
    let Some(object) = value.as_object() else {
        return Item {
            title: scalar(value).unwrap_or_else(|| summarize(value)),
            subtitle: String::new(),
            url: None,
        };
    };
    let text = |k: &str| {
        object
            .get(k)
            .and_then(scalar)
            .filter(|s| !s.trim().is_empty())
    };
    let title_key = TITLE_KEYS.iter().find(|k| text(k).is_some());
    let title = title_key
        .and_then(|k| text(k))
        .or_else(|| text("id"))
        .unwrap_or_else(|| value.to_string());
    // Short ids (issue numbers) help; UUIDs only add noise.
    let id = text("id")
        .filter(|id| id.len() <= 10)
        .map(|id| format!("#{id}"));
    let details = DETAIL_KEYS
        .iter()
        .filter(|k| Some(*k) != title_key)
        .filter_map(|k| text(k))
        .take(3);
    let subtitle = id
        .into_iter()
        .chain(details)
        .collect::<Vec<_>>()
        .join(" · ");
    let url = URL_KEYS
        .iter()
        .filter_map(|k| text(k))
        .find(|u| u.starts_with("https://") || u.starts_with("http://"));
    Item {
        title: title.lines().next().unwrap_or_default().to_owned(),
        subtitle: subtitle.lines().next().unwrap_or_default().to_owned(),
        url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn output(stdout: Value) -> Output {
        Output {
            stdout: stdout.to_string().into_bytes(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn catalog_keeps_read_commands_without_required_args() {
        let catalog = json!({"schema": "demo/v1", "ok": true, "data": {"commands": [
            {"name": "inbox", "safety": "read", "summary": "Notifications", "args": []},
            {"name": "issue list", "safety": "read", "summary": "Issues",
             "args": [{"name": "--mine", "required": false}]},
            {"name": "issue view", "safety": "read", "args": [{"name": "<id>", "required": true}]},
            {"name": "issue comment", "safety": "write", "args": []},
            {"name": "commands", "safety": "read", "args": []}
        ]}});
        let commands = parse_catalog("demo", Path::new("/bin/demo"), &catalog);
        let titles: Vec<String> = commands.iter().map(PluginCommand::title).collect();
        assert_eq!(titles, ["demo inbox", "demo issue list"]);
        assert_eq!(commands[1].words, ["issue", "list"]);
        assert_eq!(commands[0].summary, "Notifications");
    }

    #[test]
    fn list_results_become_rows_with_safe_links_only() {
        let rows = parse_output(&output(
            json!({"ok": true, "data": {"count": 2, "results": [
                {"id": "101", "title": "Fix login", "status": "New", "assignee": "Asha",
                 "url": "https://tracker.example/issues/101"},
                {"id": "102", "title": "Line one\nline two", "url": "javascript:alert(1)"}
            ]}}),
        ))
        .unwrap();
        assert_eq!(
            rows[0],
            Item {
                title: "Fix login".into(),
                subtitle: "#101 · New · Asha".into(),
                url: Some("https://tracker.example/issues/101".into()),
            }
        );
        assert_eq!(rows[1].title, "Line one");
        assert_eq!(rows[1].url, None);
    }

    #[test]
    fn items_key_wins_and_uuid_ids_are_hidden() {
        let rows = items(&json!({"has_more": false, "items": [
            {"id": "01a2-0d0e-ef8c-7605-83d0", "title": "Someone updated #7", "body": "Sprint",
             "unread": {"total": 3}}
        ], "unread": {"total": 3}}));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].subtitle, "Sprint");
    }

    #[test]
    fn objects_without_a_list_become_key_value_rows() {
        let rows = items(&json!({"total": 14, "split": {"mine": 2}, "tags": ["a", "b"]}));
        let pairs: Vec<(String, String)> =
            rows.into_iter().map(|r| (r.title, r.subtitle)).collect();
        assert!(pairs.contains(&("total".into(), "14".into())));
        assert!(pairs.contains(&("split.mine".into(), "2".into())));
        assert!(pairs.contains(&("tags".into(), "2 items".into())));
    }

    #[test]
    fn error_envelopes_and_crashes_become_messages() {
        let failed = parse_output(&output(json!({"ok": false,
            "error": {"code": "AUTH_MISSING", "message": "Not signed in"}})));
        assert_eq!(failed, Err("Not signed in (AUTH_MISSING)".into()));
        let crashed = parse_output(&Output {
            stdout: Vec::new(),
            stderr: b"thread panicked\nboom: broken\n\n".to_vec(),
        });
        assert_eq!(crashed, Err("boom: broken".into()));
    }

    #[test]
    fn filter_matches_every_word_in_any_order() {
        let rows = vec![
            Item {
                title: "Fix login page".into(),
                subtitle: "New".into(),
                url: None,
            },
            Item {
                title: "Exam results".into(),
                subtitle: "Closed".into(),
                url: None,
            },
        ];
        assert_eq!(filter("page fix", &rows), [0]);
        assert_eq!(filter("CLOSED", &rows), [1]);
        assert_eq!(filter("", &rows), [0, 1]);
    }

    #[test]
    fn manifests_drive_discovery_and_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("demo-cli");
        std::fs::write(
            &script,
            "#!/bin/sh\n[ \"$1\" = commands ] && echo '{\"ok\":true,\"data\":{\"commands\":[{\"name\":\"ping\",\"safety\":\"read\",\"args\":[]}]}}'\n[ \"$1\" = ping ] && echo '{\"ok\":true,\"data\":{\"pong\":1}}'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plugins = dir.path().join("plugins");
        std::fs::create_dir(&plugins).unwrap();
        std::fs::write(
            plugins.join("demo.json"),
            json!({"program": script}).to_string(),
        )
        .unwrap();
        std::fs::write(plugins.join("relative.json"), r#"{"program": "mytool"}"#).unwrap();

        let commands = discover(&plugins);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].title(), "demo-cli ping");
        let rows = commands[0].run().unwrap();
        assert_eq!(
            (rows[0].title.as_str(), rows[0].subtitle.as_str()),
            ("pong", "1")
        );
        assert_eq!(fingerprint(&plugins).len(), 3);
    }
}

//! gray-prompt — layered user prompt customization.
//!
//! `prompt/context` injects every customization file that exists —
//! `<gray-home>/prompt/custom.md` (global) and
//! `<session.cwd>/.gray-prompt.md` (per-project) — each capped at 8 KiB.
//! `/prompt` lists what the last injection actually sent.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Value, json};

/// Per-file byte cap.
const CAP: usize = 8 * 1024;

/// What the most recent `prompt/context` call injected: `(source, bytes)`.
static LAST: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());

fn manifest() -> Value {
    json!({
        "name": "prompt",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": "1.1",
        "tools": [],
        "commands": ["/prompt"],
        "hooks": ["prompt/context"],
    })
}

/// `<gray-home>/prompt/custom.md` — honors `$GRAY_HOME`, falls back to
/// `$HOME/.gray`.
fn global_file() -> PathBuf {
    std::env::var_os("GRAY_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".gray")))
        .unwrap_or_else(|| PathBuf::from(".gray"))
        .join("prompt")
        .join("custom.md")
}

/// The customization files, in injection order: global, then per-project.
fn candidates(cwd: &Path) -> Vec<PathBuf> {
    vec![global_file(), cwd.join(".gray-prompt.md")]
}

fn session_cwd(params: &Value) -> PathBuf {
    params["session"]["cwd"]
        .as_str()
        .or_else(|| params["cwd"].as_str())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Truncate at `max` bytes on a char boundary.
fn truncate_utf8(s: &mut String, max: usize) {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
}

/// `prompt/context`: inject every customization file that exists. Also
/// records the injection for `/prompt`.
fn prompt_context(cwd: &Path) -> Value {
    let mut out = String::new();
    let mut injected: Vec<(String, usize)> = Vec::new();
    for file in candidates(cwd) {
        let Ok(mut body) = std::fs::read_to_string(&file) else { continue };
        if body.trim().is_empty() {
            continue;
        }
        truncate_utf8(&mut body, CAP);
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("## ");
        out.push_str(&file.display().to_string());
        out.push_str("\n\n");
        out.push_str(body.trim_end());
        injected.push((file.display().to_string(), body.len()));
    }
    *LAST.lock().unwrap() = injected;
    if out.is_empty() { json!({}) } else { json!({ "text": out }) }
}

/// `/prompt` — what the last `prompt/context` call injected.
fn run_command() -> String {
    let last = LAST.lock().unwrap();
    if last.is_empty() {
        return format!(
            "nothing injected yet — looked for {} and <cwd>/.gray-prompt.md",
            global_file().display()
        );
    }
    let mut lines = vec!["last prompt/context injected:".to_string()];
    for (path, size) in last.iter() {
        lines.push(format!("  {path} ({size} B, cap {CAP} B)"));
    }
    lines.join("\n")
}

/// One request → `Some(reply)`, or `None` for notifications. The bool asks
/// the loop to exit after writing the reply.
fn handle(req: &Value) -> (Option<Value>, bool) {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let Some(id) = id else {
        return (None, method == "plugin/shutdown");
    };
    let result = match method {
        "plugin/manifest" => manifest(),
        "prompt/context" => prompt_context(&session_cwd(&params)),
        "command/run" => json!({ "text": run_command() }),
        "plugin/shutdown" => return (Some(json!({ "id": id, "result": {} })), true),
        _ => {
            let error = json!({ "code": -32601, "message": "method not found" });
            return (Some(json!({ "id": id, "error": error })), false);
        }
    };
    (Some(json!({ "id": id, "result": result })), false)
}

fn main() -> std::io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("manifest") {
        println!("{}", manifest());
        return Ok(());
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
        let (reply, exit) = handle(&req);
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
        if exit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, params: Value) -> Value {
        handle(&json!({ "id": 1, "method": method, "params": params })).0.unwrap()
    }

    /// A fresh temp dir that removes itself on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "gray-prompt-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Env vars are process-global: serialize the tests that set GRAY_HOME.
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    struct EnvGuard;
    impl EnvGuard {
        fn set_home(dir: &TempDir) -> Self {
            unsafe { std::env::set_var("GRAY_HOME", &dir.0) };
            Self
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var("GRAY_HOME") };
        }
    }

    #[test]
    fn manifest_claims_prompt_context_and_prompt_command() {
        let m = call("plugin/manifest", Value::Null)["result"].clone();
        assert_eq!(m["name"], "prompt");
        assert_eq!(m["hooks"], json!(["prompt/context"]));
        assert_eq!(m["commands"], json!(["/prompt"]));
    }

    #[test]
    fn injects_nothing_when_no_files() {
        let _lock = ENV_LOCK.lock().unwrap();
        let home = TempDir::new();
        let _g = EnvGuard::set_home(&home);
        let cwd = TempDir::new();
        let r = call("prompt/context", json!({ "session": { "cwd": cwd.0 } }));
        assert_eq!(r["result"], json!({}));
        let r = call("command/run", json!({ "name": "/prompt", "argv": [] }));
        assert!(r["result"]["text"]
            .as_str()
            .unwrap()
            .contains("nothing injected"));
    }

    #[test]
    fn injects_global_and_project_files() {
        let _lock = ENV_LOCK.lock().unwrap();
        let home = TempDir::new();
        let _g = EnvGuard::set_home(&home);
        std::fs::create_dir_all(home.0.join("prompt")).unwrap();
        std::fs::write(home.0.join("prompt/custom.md"), "global prefs").unwrap();
        let cwd = TempDir::new();
        std::fs::write(cwd.0.join(".gray-prompt.md"), "project prefs").unwrap();
        let r = call("prompt/context", json!({ "session": { "cwd": cwd.0 } }));
        let text = r["result"]["text"].as_str().unwrap();
        assert!(text.contains("global prefs") && text.contains("project prefs"));
        assert!(text.find("global prefs").unwrap() < text.find("project prefs").unwrap());
        let r = call("command/run", json!({ "name": "/prompt", "argv": [] }));
        let listed = r["result"]["text"].as_str().unwrap();
        assert!(listed.contains("custom.md") && listed.contains(".gray-prompt.md"));
    }

    #[test]
    fn caps_each_file_at_8kib() {
        let _lock = ENV_LOCK.lock().unwrap();
        let home = TempDir::new();
        let _g = EnvGuard::set_home(&home);
        let cwd = TempDir::new();
        std::fs::write(cwd.0.join(".gray-prompt.md"), "x".repeat(CAP + 4096)).unwrap();
        let r = call("prompt/context", json!({ "session": { "cwd": cwd.0 } }));
        let text = r["result"]["text"].as_str().unwrap();
        assert!(text.matches('x').count() <= CAP);
    }

    #[test]
    fn shutdown_replies_then_exits_and_notifications_are_silent() {
        let (reply, exit) = handle(&json!({ "id": 2, "method": "plugin/shutdown" }));
        assert!(reply.is_some() && exit);
        let (reply, exit) = handle(&json!({ "method": "plugin/shutdown" }));
        assert!(reply.is_none() && exit);
    }

    #[test]
    fn unknown_methods_are_method_not_found() {
        assert_eq!(call("nope", Value::Null)["error"]["code"], -32601);
    }
}

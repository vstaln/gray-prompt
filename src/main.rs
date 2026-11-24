//!
//!
//!
//! `input/submit` is a CHAIN in fixed order:
//!   commands → preset → transform → inline-bash
//! Each stage gets the text the previous stage produced. `{text}` rewrites
//! the input and moves on, `{handled: true}` swallows it and stops the
//!

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use serde_json::{Value, json};

mod parts;

#[allow(dead_code)]
#[path = "mods/commands.rs"]
mod commands;
#[allow(dead_code)]
#[path = "mods/custom.rs"]
mod custom;
#[allow(dead_code)]
#[path = "mods/inlinebash.rs"]
mod inlinebash;
#[allow(dead_code)]
#[path = "mods/preset.rs"]
mod preset;
#[allow(dead_code)]
#[path = "mods/transform.rs"]
mod transform;

/// host/say wait (transform + inline-bash used 10s).
const SAY_TTL: Duration = Duration::from_secs(10);
/// host/ask wait (commands + preset used 300s, under the host's 330s TTL).
const ASK_TTL: Duration = Duration::from_secs(300);

const PARTS: &[parts::Part] = &[
    parts::Part {
        name: "custom",
        default: true,
        description: "layered prompt files: ~/.gray/prompt/custom.md and <cwd>/.gray-prompt.md",
    },
    parts::Part {
        name: "commands",
        default: false,
        description: "prompt templates from ~/.gray/commands",
    },
    parts::Part {
        name: "preset",
        default: false,
        description: "working-preference bundles from ~/.gray/presets",
    },
    parts::Part {
        name: "transform",
        default: false,
        description: "quick-word rewrites and rules from ~/.gray/transform",
    },
    parts::Part {
        name: "inline-bash",
        default: false,
        description: "run `!cmd` inline from the input box",
    },
];

fn union_strs(acc: &mut Vec<Value>, m: &Value, key: &str) {
    for v in m.get(key).and_then(Value::as_array).into_iter().flatten() {
        if !acc.contains(v) {
            acc.push(v.clone());
        }
    }
}

fn union_map(acc: &mut serde_json::Map<String, Value>, m: &Value, key: &str) {
    if let Some(map) = m.get(key).and_then(Value::as_object) {
        for (k, v) in map {
            acc.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
}

fn manifest(parts: &parts::Parts) -> Value {
    let mut commands: Vec<Value> = Vec::new();
    let mut hooks: Vec<Value> = Vec::new();
    let mut capabilities: Vec<Value> = Vec::new();
    let mut descriptions = serde_json::Map::new();
    let owned: [(&str, Value); 5] = [
        ("custom", custom::manifest_custom()),
        ("commands", commands::manifest_commands()),
        ("preset", preset::manifest_preset()),
        ("transform", transform::manifest_transform()),
        ("inline-bash", inlinebash::manifest_inlinebash()),
    ];
    for (name, m) in &owned {
        if !parts.on(name) {
            continue;
        }
        union_strs(&mut commands, m, "commands");
        union_strs(&mut hooks, m, "hooks");
        union_strs(&mut capabilities, m, "capabilities");
        union_map(&mut descriptions, m, "command_descriptions");
    }
    json!({
        "name": "prompt",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": "2.0",
        "tools": [],
        "description": "layered prompt customization and input shaping",
        "commands": commands,
        "command_descriptions": descriptions,
        "hooks": hooks,
        "capabilities": capabilities,
        "parts": parts.manifest(PARTS),
    })
}

type Pending = Arc<Mutex<HashMap<String, mpsc::Sender<Value>>>>;

/// Sidecar→host request channel shared by every module.
struct Host {
    out: Arc<Mutex<std::io::Stdout>>,
    pending: Pending,
    counter: Mutex<u64>,
}

impl Host {
    /// Send `method` and wait up to `ttl` for the full reply frame.
    fn call(&self, method: &str, params: Value, ttl: Duration) -> Result<Value, String> {
        let id = {
            let mut n = self.counter.lock().expect("id counter");
            *n += 1;
            format!("q{n}")
        };
        let (tx, rx) = mpsc::channel();
        self.pending.lock().expect("pending").insert(id.clone(), tx);
        {
            let req = json!({ "id": id, "method": method, "params": params });
            let mut o = self.out.lock().expect("stdout");
            if writeln!(o, "{req}").and_then(|_| o.flush()).is_err() {
                self.pending.lock().expect("pending").remove(&id);
                return Err("stdout closed".into());
            }
        }
        let reply = rx.recv_timeout(ttl);
        self.pending.lock().expect("pending").remove(&id);
        reply.map_err(|_| "host timeout".into())
    }

    /// inline-bash flavour: Err on timeout or a host error.
    fn say(&self, text: &str) -> Result<(), String> {
        let v = self.call("host/say", json!({ "text": text }), SAY_TTL)?;
        match v.get("error") {
            Some(e) => Err(e
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("host error")
                .to_string()),
            None => Ok(()),
        }
    }

    /// commands/preset flavour: the reply's `result` (Null on error), None
    /// on timeout.
    fn ask(&self, params: Value) -> Option<Value> {
        self.call("host/ask", params, ASK_TTL)
            .ok()
            .map(|v| v.get("result").cloned().unwrap_or(Value::Null))
    }
}

/// Call one module's handler with `req`; return its `result` object, or
/// None for an error/no reply (a stage that doesn't handle the method).
fn stage_result(host: &Host, module: &str, req: &Value) -> Option<Value> {
    let (reply, _) = dispatch(host, module, req);
    let reply = reply?;
    if reply.get("error").is_some() {
        return None;
    }
    reply.get("result").cloned()
}

/// Run one module's own `handle_<mod>` with the host channels it expects.
fn dispatch(host: &Host, module: &str, req: &Value) -> (Option<Value>, bool) {
    match module {
        "custom" => custom::handle_custom(req),
        "commands" => {
            let dir = commands::commands_dir();
            let mut ask = |qs: &[Value]| host.ask(json!({ "questions": qs, "blocking": true }));
            commands::handle_commands(req, &dir, &mut ask)
        }
        "preset" => {
            let mut ask = |p: &Value| host.ask(p.clone());
            preset::handle_preset(req, &mut ask)
        }
        "transform" => {
            let rules = transform::load_rules();
            let mut say = |t: &str| {
                let _ = host.say(t);
            };
            transform::handle_transform(req, &mut say, &rules)
        }
        "inlinebash" => {
            let mut say = |t: &str| host.say(t);
            inlinebash::handle_inlinebash(req, &mut say)
        }
        _ => (None, false),
    }
}

/// Fixed `input/submit` chain order.
const SUBMIT_CHAIN: [&str; 4] = ["commands", "preset", "transform", "inlinebash"];

const MODULE_PARTS: [(&str, &str); 5] = [
    ("custom", "custom"),
    ("commands", "commands"),
    ("preset", "preset"),
    ("transform", "transform"),
    ("inlinebash", "inline-bash"),
];

fn module_part(module: &str) -> &str {
    MODULE_PARTS
        .iter()
        .find(|(m, _)| *m == module)
        .map_or("", |(_, part)| *part)
}

fn input_submit(host: &Host, req: &Value, parts: &parts::Parts) -> Value {
    let mut params = req.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        params = json!({});
    }
    let original = params
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut text = original.clone();
    for module in SUBMIT_CHAIN {
        if !parts.on(module_part(module)) {
            continue;
        }
        params["text"] = Value::String(text.clone());
        let stage_req = json!({ "id": req.get("id").cloned().unwrap_or(json!(0)),
                                "method": "input/submit", "params": params });
        let Some(result) = stage_result(host, module, &stage_req) else {
            continue;
        };
        if result.get("handled").and_then(Value::as_bool) == Some(true) {
            return json!({ "handled": true });
        }
        if let Some(t) = result.get("text").and_then(Value::as_str) {
            text = t.to_string();
        }
    }
    if text == original {
        json!({})
    } else {
        json!({ "text": text })
    }
}

fn command_owner(name: &str, parts: &parts::Parts) -> Option<&'static str> {
    let module = match name {
        "/prompt" => "custom",
        "/cmd" => "commands",
        "/preset" => "preset",
        "/transform" => "transform",
        "/inline" => "inlinebash",
        _ => return None,
    };
    parts.on(module_part(module)).then_some(module)
}

fn handle(host: &Host, req: &Value, parts: &parts::Parts) -> (Option<Value>, bool) {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let Some(id) = id else {
        return (None, method == "plugin/shutdown");
    };
    let result = match method {
        "plugin/manifest" => manifest(parts),
        "prompt/context" => {
            if parts.on("custom") {
                stage_result(host, "custom", req).unwrap_or_else(|| json!({}))
            } else {
                json!({})
            }
        }
        "input/submit" => {
            if !parts.on("transform") && !parts.on("inline-bash") {
                return (Some(json!({ "id": id, "result": {} })), false);
            }
            input_submit(host, req, parts)
        }
        "command/run" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            if let Some(module) = command_owner(name, parts) {
                stage_result(host, module, req).unwrap_or_else(|| json!({}))
            } else {
                json!({ "text": format!("unknown command {name}") })
            }
        }
        "plugin/shutdown" => return (Some(json!({ "id": id, "result": {} })), true),
        _ => {
            let error = json!({ "code": -32601, "message": "method not found" });
            return (Some(json!({ "id": id, "error": error })), false);
        }
    };
    (Some(json!({ "id": id, "result": result })), false)
}

fn main() -> std::io::Result<()> {
    let parts = parts::Parts::load("prompt", PARTS);
    if std::env::args().nth(1).as_deref() == Some("manifest") {
        println!("{}", manifest(&parts));
        return Ok(());
    }
    let host = Host {
        out: Arc::new(Mutex::new(std::io::stdout())),
        pending: Arc::new(Mutex::new(HashMap::new())),
        counter: Mutex::new(0),
    };

    let (work_tx, work_rx) = mpsc::channel::<Value>();
    let reader_pending = host.pending.clone();
    let _reader = std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if v.get("method").is_none() {
                if let Some(id) = v.get("id").and_then(Value::as_str)
                    && let Some(tx) = reader_pending.lock().expect("pending").remove(id)
                {
                    let _ = tx.send(v);
                }
                continue;
            }
            if work_tx.send(v).is_err() {
                break;
            }
        }
    });

    for req in work_rx {
        let (reply, exit) = handle(&host, &req, &parts);
        if let Some(reply) = reply {
            let mut o = host.out.lock().expect("stdout");
            let _ = writeln!(o, "{reply}");
            let _ = o.flush();
        }
        if exit {
            break;
        }
    }
    // Detached stdin pump; returning ends the process with it.
    Ok(())
}

//! 内置前端界面：一个只监听本机回环地址的极简 HTTP 服务，外加内嵌的单页界面。
//!
//! 设计取舍：
//!   * 不引入任何 GUI 框架 / 第三方 crate，保持"单文件 exe、零运行时依赖"
//!   * 只绑定 127.0.0.1，不对外网暴露；不写 cookie、不做鉴权（本机单用户工具）
//!   * 界面的所有动作都直接调用与 CLI 相同的引擎函数，两条入口行为一致

use crate::config::{self, Config, EntryKind, VaultSpec};
use crate::{
    discover_config, link_vault, load_config, rel_to_native, remove_link, verify_into, Existing,
    Fate, Options, Output, Report,
};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const DEFAULT_PORT: u16 = 7411;
const PORT_TRIES: u16 = 20;

/// 图形界面是否可用（用于 main.rs 的启动自检）。
pub fn capability() -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| format!("无法在本机回环地址上监听端口：{e}"))?;
    drop(listener);
    Ok(())
}

/// `gui` 子命令的选项。
pub struct GuiOptions {
    pub port: u16,
    pub open_browser: bool,
    pub config_path: Option<PathBuf>,
    pub root: Option<PathBuf>,
}

impl Default for GuiOptions {
    fn default() -> Self {
        GuiOptions {
            port: DEFAULT_PORT,
            open_browser: true,
            config_path: None,
            root: None,
        }
    }
}

impl GuiOptions {
    /// 转成引擎使用的选项（GUI 里所有动作都等价于 `--yes` 的非交互执行）。
    fn engine_options(&self) -> Options {
        Options {
            command: String::new(),
            positional: Vec::new(),
            config_path: self.config_path.clone(),
            root: self.root.clone(),
            dry_run: false,
            force: false,
            no_verify: false,
            verbose: false,
            yes: true,
            port: self.port,
            no_browser: !self.open_browser,
        }
    }
}

/// 启动图形界面，阻塞直到进程被中断。
pub fn serve(opts: GuiOptions) -> Result<(), String> {
    capability()?;
    // 启动前先确认配置可用，避免界面起来了却什么都做不了
    let cfg = load_config(&opts.engine_options())?;

    let (listener, port) = bind(opts.port)?;
    let url = format!("http://127.0.0.1:{port}/");

    println!("obsidian-sync 图形界面已启动");
    println!("  地址：{url}");
    println!("  仓库集合：{}", cfg.root.display());
    println!("  共享母本：{}", cfg.shared_dir.display());
    println!("\n按 Ctrl+C 结束。");

    if opts.open_browser {
        open_url(&url);
    }

    let state = Arc::new(AppState {
        config_path: opts.config_path.clone(),
        root: opts.root.clone(),
        needed_repair: AtomicBool::new(false),
    });

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                // 单用户本机工具：每个连接起一个线程，够用且实现简单
                std::thread::spawn(move || {
                    let _ = handle_connection(stream, &state);
                });
            }
            Err(_) => continue,
        }
    }
    Ok(())
}

fn bind(preferred: u16) -> Result<(TcpListener, u16), String> {
    for offset in 0..PORT_TRIES {
        let port = preferred.saturating_add(offset);
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
            return Ok((listener, port));
        }
    }
    Err(format!(
        "端口 {preferred} 起的 {PORT_TRIES} 个端口都被占用，请用 --port 指定其他端口。"
    ))
}

struct AppState {
    config_path: Option<PathBuf>,
    root: Option<PathBuf>,
    /// 最近一次 check 是否发现需要修复的问题（供界面决定是否高亮"建立/修复"）
    needed_repair: AtomicBool,
}

impl AppState {
    fn options(&self) -> Options {
        Options {
            command: String::new(),
            positional: Vec::new(),
            config_path: self.config_path.clone(),
            root: self.root.clone(),
            dry_run: false,
            force: false,
            no_verify: false,
            verbose: false,
            yes: true,
            port: 0,
            no_browser: true,
        }
    }

    fn load(&self) -> Result<Config, String> {
        load_config(&self.options())
    }
}

fn open_url(url: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

// ------------------------------------------------------------------ HTTP

struct Request {
    method: String,
    path: String,
    body: String,
}

fn handle_connection(mut stream: TcpStream, state: &AppState) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let raw_path = parts.next().unwrap_or("/").to_string();

    // 读取请求头，拿到 Content-Length
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }

    let mut body = String::new();
    if content_length > 0 {
        let mut buf = vec![0u8; content_length.min(1 << 20)];
        reader.read_exact(&mut buf)?;
        body = String::from_utf8_lossy(&buf).to_string();
    }

    let path = raw_path.split('?').next().unwrap_or("/").to_string();
    let request = Request { method, path, body };

    let (status, content_type, payload) = route(&request, state);
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        payload.as_bytes().len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(payload.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn route(req: &Request, state: &AppState) -> (&'static str, &'static str, String) {
    let json = "application/json; charset=utf-8";
    let html = "text/html; charset=utf-8";

    if req.method == "GET" && (req.path == "/" || req.path == "/index.html") {
        return ("200 OK", html, INDEX_HTML.to_string());
    }

    if req.method == "GET" && req.path == "/api/status" {
        return match build_status(state) {
            Ok(v) => ("200 OK", json, v.to_json()),
            Err(e) => ("500 Internal Server Error", json, error_json(&e)),
        };
    }

    if req.method == "POST" {
        let action = req.path.trim_start_matches("/api/").to_string();
        return match run_action(&action, &req.body, state) {
            Ok(v) => ("200 OK", json, v.to_json()),
            Err(e) => ("200 OK", json, error_json(&e)),
        };
    }

    ("404 Not Found", json, error_json("未知路径"))
}

fn error_json(message: &str) -> String {
    Json::obj(vec![("ok", Json::bool(false)), ("error", Json::str(message))]).to_json()
}

// ------------------------------------------------------------------ 动作

fn run_action(action: &str, body: &str, state: &AppState) -> Result<Json, String> {
    let cfg = state.load()?;
    let vaults_arg = json_get_string(body, "vaults").unwrap_or_default();
    let names: Vec<String> = vaults_arg
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let selected: Vec<VaultSpec> = if names.is_empty() {
        cfg.vaults.clone()
    } else {
        let mut out = Vec::new();
        for n in &names {
            let v = cfg
                .vault_by_name(n)
                .ok_or_else(|| format!("配置里没有名为 {n} 的库"))?;
            out.push(v.clone());
        }
        out
    };

    let mut report = Report::default();

    match action {
        "check" => {
            let refs: Vec<&VaultSpec> = selected.iter().collect();
            verify_into(&cfg, &refs, &mut report, true);
            let fails = report.failures();
            state.needed_repair.store(fails > 0, Ordering::SeqCst);
        }
        "link" => {
            let force = json_get_bool(body, "force").unwrap_or(false);
            let opts = Options {
                force,
                ..state.options()
            };
            for vault in &selected {
                link_vault(&cfg, vault, &opts, &Output::silent(), &mut report)?;
            }
            let refs: Vec<&VaultSpec> = selected.iter().collect();
            verify_into(&cfg, &refs, &mut report, false);
            state.needed_repair.store(false, Ordering::SeqCst);
        }
        "unlink" => {
            for vault in &selected {
                for entry in cfg.entries_for(vault).entries() {
                    let kind = entry.kind.resolve(&entry.rel);
                    let target = cfg.shared_dir.join(rel_to_native(&entry.rel));
                    let link_path = vault.path.join(rel_to_native(&entry.rel));
                    let (fate, method, detail) = match crate::inspect(&link_path, &target, kind) {
                        Existing::Link { method } => match remove_link(&link_path, method) {
                            Ok(()) => (
                                Fate::Remove,
                                method.label().to_string(),
                                "已移除".to_string(),
                            ),
                            Err(e) => (Fate::Fail, String::new(), e),
                        },
                        Existing::BrokenLink { .. } => (Fate::Skip, String::new(), "链接已失效，跳过".into()),
                        Existing::Real => (
                            Fate::Skip,
                            String::new(),
                            "是真实文件/目录，未改动".into(),
                        ),
                        Existing::Missing => (Fate::Skip, String::new(), "本来就不存在".into()),
                    };
                    report.push(crate::Record {
                        vault: vault.name.clone(),
                        rel: entry.rel.clone(),
                        fate,
                        method,
                        detail,
                    });
                }
            }
        }
        "add" => {
            let path_text = json_get_string(body, "path")
                .ok_or("缺少参数 path（要纳入同步的仓库目录）")?;
            let name = json_get_string(body, "name").filter(|s| !s.trim().is_empty());
            let profile = json_get_string(body, "profile").filter(|s| !s.trim().is_empty());
            add_vault(&cfg, &path_text, name, profile, &mut report)?;
            let cfg2 = state.load()?;
            let fresh: Vec<VaultSpec> = cfg2
                .vaults
                .iter()
                .filter(|v| !cfg.vaults.iter().any(|o| o.name == v.name))
                .cloned()
                .collect();
            let opts = state.options();
            for vault in &fresh {
                link_vault(&cfg2, vault, &opts, &Output::silent(), &mut report)?;
            }
        }
        "open" => {
            let name = names.first().ok_or("缺少库名")?;
            let vault = cfg
                .vault_by_name(name)
                .ok_or_else(|| format!("配置里没有名为 {name} 的库"))?;
            let uri = format!(
                "obsidian://open?vault={}",
                urlencode(&vault.path.file_name().unwrap_or_default().to_string_lossy())
            );
            open_url(&uri);
            report.push(crate::Record {
                vault: vault.name.clone(),
                rel: "obsidian://".into(),
                fate: Fate::Keep,
                method: String::new(),
                detail: format!("已请求 Obsidian 打开：{uri}"),
            });
        }
        other => return Err(format!("未知操作：{other}")),
    }

    Ok(Json::obj(vec![
        ("ok", Json::bool(report.failures() == 0)),
        ("report", report_json(&report)),
        ("status", build_status(state)?),
    ]))
}

/// 把一个新仓库登记进 sync.toml（与 CLI 的 `new` 行为一致）。
fn add_vault(
    cfg: &Config,
    path_text: &str,
    name: Option<String>,
    profile: Option<String>,
    report: &mut Report,
) -> Result<(), String> {
    let raw = PathBuf::from(path_text);
    let vault_path = config::absolutize(&raw, &cfg.root);
    if !vault_path.is_dir() {
        // 界面上明确说过"还不是 Obsidian 库会自动创建"，这里兑现该承诺：
        // 目录本身不存在时先建出来，并在 .obsidian 缺失时补一个空目录，
        // 让后续的链接有地方安放（Obsidian 之后打开会自行补全其余默认设置）。
        std::fs::create_dir_all(&vault_path)
            .map_err(|e| format!("创建目录失败 {}：{e}", vault_path.display()))?;
        report.push(crate::Record {
            vault: vault_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "vault".into()),
            rel: "库目录".into(),
            fate: Fate::Create,
            method: String::new(),
            detail: format!("目录不存在，已创建：{}", vault_path.display()),
        });
    }
    let obsidian_dir = vault_path.join(".obsidian");
    if !obsidian_dir.is_dir() {
        std::fs::create_dir_all(&obsidian_dir)
            .map_err(|e| format!("创建 .obsidian 失败：{e}"))?;
        report.push(crate::Record {
            vault: vault_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "vault".into()),
            rel: ".obsidian".into(),
            fate: Fate::Create,
            method: String::new(),
            detail: "不是 Obsidian 库，已创建空的 .obsidian 目录".into(),
        });
    }
    let vault_name = name.unwrap_or_else(|| {
        vault_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "vault".into())
    });

    if cfg.vault_by_name(&vault_name).is_some() {
        report.push(crate::Record {
            vault: vault_name.clone(),
            rel: "sync.toml".into(),
            fate: Fate::Skip,
            method: String::new(),
            detail: "该库已在配置中，跳过写入".into(),
        });
        return Ok(());
    }

    let rel = crate::relative_to(&cfg.root, &vault_path);
    let line = match &profile {
        Some(p) => format!("\"{vault_name}\" = {{ path = \"{rel}\", profile = \"{p}\" }}\n"),
        None => format!("\"{vault_name}\" = {{ path = \"{rel}\" }}\n"),
    };
    let mut text = std::fs::read_to_string(&cfg.config_path)
        .map_err(|e| format!("读取配置失败：{e}"))?;
    if !text.contains("[vaults]") {
        text.push_str("\n[vaults]\n");
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&line);
    std::fs::write(&cfg.config_path, text).map_err(|e| format!("写入配置失败：{e}"))?;

    report.push(crate::Record {
        vault: vault_name,
        rel: "sync.toml".into(),
        fate: Fate::Create,
        method: String::new(),
        detail: format!("已登记：{line}"),
    });
    Ok(())
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ------------------------------------------------------------------ 状态快照

fn build_status(state: &AppState) -> Result<Json, String> {
    let cfg = state.load()?;
    let symlink_ok = symlink_probe();

    let mut vaults = Vec::new();
    for vault in &cfg.vaults {
        let entries = cfg.entries_for(vault).entries();
        let exists = vault.path.is_dir();
        let mut links = Vec::new();
        let mut ok = 0usize;
        let mut bad = 0usize;

        for entry in &entries {
            let kind = entry.kind.resolve(&entry.rel);
            let target = cfg.shared_dir.join(rel_to_native(&entry.rel));
            let link_path = vault.path.join(rel_to_native(&entry.rel));
            let (state_text, detail) = if !exists {
                ("missing".to_string(), "库目录不存在".to_string())
            } else if !target.exists() {
                ("source-missing".to_string(), "共享母本里没有这一项".to_string())
            } else {
                match crate::inspect(&link_path, &target, kind) {
                    Existing::Link { method } => {
                        ok += 1;
                        ("ok".to_string(), method.label().to_string())
                    }
                    Existing::BrokenLink { current } => {
                        bad += 1;
                        (
                            "broken".to_string(),
                            match current {
                                Some(c) => format!("指向 {}", c.display()),
                                None => "链接无法解析".to_string(),
                            },
                        )
                    }
                    Existing::Real => {
                        bad += 1;
                        ("real".to_string(), "是真实文件/目录，不是链接".to_string())
                    }
                    Existing::Missing => {
                        bad += 1;
                        ("missing".to_string(), "链接缺失".to_string())
                    }
                }
            };
            links.push(Json::obj(vec![
                ("rel", Json::str(&entry.rel)),
                ("kind", Json::str(if kind == EntryKind::Dir { "dir" } else { "file" })),
                ("state", Json::str(&state_text)),
                ("detail", Json::str(&detail)),
            ]));
        }

        let status = if !exists {
            "error"
        } else if bad == 0 {
            "ok"
        } else {
            "warn"
        };

        vaults.push(Json::obj(vec![
            ("name", Json::str(&vault.name)),
            ("path", Json::str(&vault.path.to_string_lossy())),
            ("profile", Json::str(vault.profile.as_deref().unwrap_or(""))),
            ("status", Json::str(status)),
            ("total", Json::num(entries.len() as f64)),
            ("ok", Json::num(ok as f64)),
            ("bad", Json::num(bad as f64)),
            ("links", Json::arr(links)),
        ]));
    }

    let profiles: Vec<Json> = cfg
        .profiles
        .keys()
        .map(|k| Json::str(k))
        .collect();

    Ok(Json::obj(vec![
        ("ok", Json::bool(true)),
        ("config", Json::str(&cfg.config_path.to_string_lossy())),
        ("root", Json::str(&cfg.root.to_string_lossy())),
        ("sharedDir", Json::str(&cfg.shared_dir.to_string_lossy())),
        ("sharedOk", Json::bool(cfg.shared_dir.is_dir())),
        ("symlinkOk", Json::bool(symlink_ok)),
        ("neededRepair", Json::bool(state.needed_repair.load(Ordering::SeqCst))),
        ("profiles", Json::arr(profiles)),
        ("vaults", Json::arr(vaults)),
    ]))
}

fn symlink_probe() -> bool {
    let dir = std::env::temp_dir().join(format!("obsidian-sync-probe-{}", std::process::id()));
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let target = dir.join("t.txt");
    let link = dir.join("l.txt");
    let ok = std::fs::write(&target, b"probe").is_ok()
        && std::os::windows::fs::symlink_file(&target, &link).is_ok();
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_dir_all(&dir);
    ok
}

fn report_json(report: &Report) -> Json {
    let items: Vec<Json> = report
        .records
        .iter()
        .map(|r| {
            Json::obj(vec![
                ("vault", Json::str(&r.vault)),
                ("rel", Json::str(&r.rel)),
                ("fate", Json::str(r.fate.tag())),
                (
                    "level",
                    Json::str(match r.fate {
                        Fate::Fail => "error",
                        Fate::Create | Fate::Recreate | Fate::Remove => "change",
                        Fate::Skip => "skip",
                        Fate::Keep => "ok",
                    }),
                ),
                ("method", Json::str(&r.method)),
                ("detail", Json::str(&r.detail)),
            ])
        })
        .collect();

    Json::obj(vec![
        ("fail", Json::num(report.failures() as f64)),
        ("total", Json::num(report.records.len() as f64)),
        ("items", Json::arr(items)),
    ])
}

// ------------------------------------------------------------------ 极简 JSON

pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(&'static str, Json)>),
}

impl Json {
    pub fn bool(v: bool) -> Json {
        Json::Bool(v)
    }
    pub fn num(v: f64) -> Json {
        Json::Num(v)
    }
    pub fn str(v: &str) -> Json {
        Json::Str(v.to_string())
    }
    pub fn arr(v: Vec<Json>) -> Json {
        Json::Arr(v)
    }
    pub fn obj(v: Vec<(&'static str, Json)>) -> Json {
        Json::Obj(v)
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    out.push_str(&format!("{}", *n as i64));
                } else {
                    out.push_str(&format!("{n}"));
                }
            }
            Json::Str(s) => write_json_string(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Obj(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_string(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// 从请求体里取一个字符串字段（只支持本工具自己发出的扁平结构）。
pub fn json_get_string(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = body.find(&needle)? + needle.len();
    let rest = &body[start..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let mut value = String::new();
    let mut chars = after[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(value),
            '\\' => match chars.next() {
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                Some('r') => value.push('\r'),
                Some(other) => value.push(other),
                None => return Some(value),
            },
            other => value.push(other),
        }
    }
    Some(value)
}

pub fn json_get_bool(body: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{key}\"");
    let start = body.find(&needle)? + needle.len();
    let rest = &body[start..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    if after.starts_with("true") {
        Some(true)
    } else if after.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

// 便于上层引用而未直接使用的路径常量
#[allow(dead_code)]
pub fn default_config_name() -> &'static str {
    "sync.toml"
}

#[allow(dead_code)]
pub fn default_config_path() -> Result<PathBuf, String> {
    discover_config()
}

// ------------------------------------------------------------------ 内嵌界面

const INDEX_HTML: &str = include_str!("ui.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escapes_and_shapes() {
        let v = Json::obj(vec![
            ("a", Json::str("x\"y\\z\n")),
            ("n", Json::num(3.0)),
            ("b", Json::bool(true)),
            ("arr", Json::arr(vec![Json::str("q")])),
        ]);
        assert_eq!(
            v.to_json(),
            r#"{"a":"x\"y\\z\n","n":3,"b":true,"arr":["q"]}"#
        );
    }

    #[test]
    fn json_field_extraction() {
        let body = r#"{"vaults":"AI, Misc","force":true,"name":"x"}"#;
        assert_eq!(json_get_string(body, "vaults").unwrap(), "AI, Misc");
        assert_eq!(json_get_string(body, "name").unwrap(), "x");
        assert!(json_get_bool(body, "force").unwrap());
        assert!(json_get_string(body, "nope").is_none());
    }

    #[test]
    fn urlencode_keeps_safe_chars() {
        assert_eq!(urlencode("Obsidian-AI"), "Obsidian-AI");
        assert_eq!(urlencode("a b"), "a%20b");
    }
}

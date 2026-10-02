//! 配置模型：从 sync.toml 读取"有哪些库、共享什么、怎么链接"。

use crate::toml::{parse, Doc, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// 共享项：相对于库根目录与共享目录的同名路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// 目录（优先符号链接，失败回退目录联接）
    Dir,
    /// 文件（符号链接；失败时若内容可读则接受）
    File,
    /// 由文件扩展名决定：.json 之类按文件处理
    Auto,
}

impl EntryKind {
    /// `Auto` 按扩展名推断：有扩展名视为文件，否则视为目录。
    pub fn resolve(self, rel: &str) -> EntryKind {
        match self {
            EntryKind::Auto => {
                let name = rel.rsplit(['/', '\\']).next().unwrap_or(rel);
                if name.contains('.') {
                    EntryKind::File
                } else {
                    EntryKind::Dir
                }
            }
            other => other,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LinkEntry {
    /// 相对路径，例如 "plugins" 或 ".obsidian/app.json"
    pub rel: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Default)]
pub struct LinkSet {
    pub dirs: Vec<String>,
    pub files: Vec<String>,
    pub items: Vec<String>,
}

impl LinkSet {
    /// 与另一个集合合并，保持顺序稳定（自身在前）。
    pub fn merged_with(&self, other: &LinkSet) -> LinkSet {
        let mut out = LinkSet::default();
        out.dirs = dedup_join(&self.dirs, &other.dirs);
        out.files = dedup_join(&self.files, &other.files);
        out.items = dedup_join(&self.items, &other.items);
        out
    }

    pub fn entries(&self) -> Vec<LinkEntry> {
        let mut out = Vec::new();
        for d in &self.dirs {
            out.push(LinkEntry { rel: d.clone(), kind: EntryKind::Dir });
        }
        for f in &self.files {
            out.push(LinkEntry { rel: f.clone(), kind: EntryKind::File });
        }
        for i in &self.items {
            out.push(LinkEntry { rel: i.clone(), kind: EntryKind::Auto });
        }
        out
    }
}

fn dedup_join(a: &[String], b: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in a.iter().chain(b.iter()) {
        if !out.iter().any(|x| x.eq_ignore_ascii_case(item)) {
            out.push(item.clone());
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct Profile {
    /// true = 在默认规则之上追加；false = 完全替换默认规则
    pub extend: bool,
    pub dirs: Vec<String>,
    pub files: Vec<String>,
    pub items: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct VaultSpec {
    pub name: String,
    pub path: PathBuf,
    pub profile: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// vault 集合的根目录，用于把相对路径展开成绝对路径
    pub root: PathBuf,
    /// 共享内容母本目录
    pub shared_dir: PathBuf,
    /// 配置文件自身的位置
    pub config_path: PathBuf,
    pub defaults: LinkSet,
    pub profiles: HashMap<String, Profile>,
    pub vaults: Vec<VaultSpec>,
    pub verify: bool,
    /// 是否在覆盖真实文件前自动备份
    pub auto_backup: bool,
    pub backup_dir: PathBuf,
}

impl Config {
    pub fn vault_by_name(&self, name: &str) -> Option<&VaultSpec> {
        self.vaults.iter().find(|v| v.name.eq_ignore_ascii_case(name))
    }

    /// 某个库实际要链接的清单：默认规则 + 其 profile。
    pub fn entries_for(&self, vault: &VaultSpec) -> LinkSet {
        match vault.profile.as_ref().and_then(|p| self.profiles.get(p)) {
            Some(profile) if profile.extend => {
                let extra = LinkSet {
                    dirs: profile.dirs.clone(),
                    files: profile.files.clone(),
                    items: profile.items.clone(),
                };
                self.defaults.merged_with(&extra)
            }
            Some(profile) => LinkSet {
                dirs: profile.dirs.clone(),
                files: profile.files.clone(),
                items: profile.items.clone(),
            },
            None => self.defaults.clone(),
        }
    }

}

/// 把配置路径转成绝对路径。
fn absolute_config_path(path: &Path) -> PathBuf {
    let cleaned = strip_verbatim(path);
    match std::env::current_dir() {
        Ok(cwd) => absolutize(&cleaned, &cwd),
        Err(_) => cleaned,
    }
}

/// 去掉 `.` 与 `..` 段，输出干净的绝对路径（不访问文件系统）。
///
/// 刻意不使用 `fs::canonicalize`：Windows 上它会返回 `\\?\C:\...` 前缀路径，
/// 既不便于显示，也让路径字符串比较失配。
pub fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    // 根之前的部分（`C:` 与 `\`）不许被 `..` 弹掉，
    // 否则 `C:\a\..` 会被错误地削成 `C:`。
    let root_prefix: Vec<Component> = path
        .components()
        .take_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
        .collect();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if out.components().count() > root_prefix.len() {
                    out.pop();
                }
            }
            Component::Prefix(_) | Component::RootDir => out.push(comp.as_os_str()),
            Component::Normal(s) => out.push(s),
        }
    }
    out
}

/// 转成绝对路径（相对路径按 base 展开）并规范化。
pub fn absolutize(path: &Path, base: &Path) -> PathBuf {
    if path.is_absolute() {
        normalize(path)
    } else {
        normalize(&base.join(path))
    }
}

/// 去掉多余的 `\\?\` 前缀（用于读取外部传入的路径）。
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

fn get_str(table: &HashMap<String, Value>, key: &str) -> Option<String> {
    table.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn get_bool(table: &HashMap<String, Value>, key: &str) -> Option<bool> {
    table.get(key).and_then(|v| v.as_bool())
}

fn get_arr(table: &HashMap<String, Value>, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| a.to_vec())
        .unwrap_or_default()
}

/// 从文件加载配置。
pub fn load(config_path: &Path, root_override: Option<&Path>) -> Result<Config, String> {
    let text = fs::read_to_string(config_path)
        .map_err(|e| format!("读取配置失败 {}：{e}", config_path.display()))?;
    let doc: Doc = parse(&text)?;

    // ---- [general] ----
    let general = doc
        .section("general")
        .ok_or("配置缺少 [general] 段")?;

    let config_path = absolute_config_path(config_path);
    let cfg_dir = config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let root = match root_override {
        Some(r) => absolutize(r, &cfg_dir),
        None => match get_str(general, "root") {
            Some(r) => absolutize(Path::new(&r), &cfg_dir),
            // 未指定则取配置文件所在目录
            None => cfg_dir.clone(),
        },
    };

    let shared_raw = get_str(general, "shared_dir")
        .ok_or("[general] 缺少 shared_dir（共享内容母本目录）")?;
    let shared_dir = absolutize(Path::new(&shared_raw), &root);

    // ---- [links] ----
    let links = doc.section("links").cloned().unwrap_or_default();
    let defaults = LinkSet {
        dirs: get_arr(&links, "dirs"),
        files: get_arr(&links, "files"),
        items: get_arr(&links, "items"),
    };

    // ---- [profiles.*] ----
    let mut profiles = HashMap::new();
    for (section, table) in &doc.sections {
        if let Some(name) = section.strip_prefix("profiles.") {
            profiles.insert(
                name.to_string(),
                Profile {
                    extend: get_bool(table, "extend").unwrap_or(true),
                    dirs: get_arr(table, "dirs"),
                    files: get_arr(table, "files"),
                    items: get_arr(table, "items"),
                },
            );
        }
    }

    // ---- [vaults] ----
    let mut vaults = Vec::new();
    if let Some(table) = doc.section("vaults") {
        let mut names: Vec<&String> = table.keys().collect();
        names.sort();
        for name in names {
            let value = &table[name];
            let (raw_path, profile) = match value {
                Value::Table(t) => (
                    t.get("path").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    t.get("profile").and_then(|v| v.as_str()).map(|s| s.to_string()),
                ),
                Value::Str(s) => (Some(s.clone()), None),
                _ => (None, None),
            };
            let raw_path = raw_path.ok_or_else(|| format!("[vaults] 的 {name} 缺少 path"))?;
            vaults.push(VaultSpec {
                name: name.clone(),
                path: absolutize(Path::new(&raw_path), &root),
                profile,
            });
        }
    }

    let backup_dir = match get_str(general, "backup_dir") {
        Some(b) => absolutize(Path::new(&b), &root),
        None => normalize(&root.join("_backup")),
    };

    Ok(Config {
        root,
        shared_dir,
        config_path,
        defaults,
        profiles,
        vaults,
        verify: get_bool(general, "verify").unwrap_or(true),
        auto_backup: get_bool(general, "auto_backup").unwrap_or(true),
        backup_dir,
    })
}

#[cfg(test)]
mod path_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn normalize_handles_parent_dirs() {
        assert_eq!(
            normalize(Path::new(r"C:\ObsidianVault\Obsidian-SettingSync\..")),
            PathBuf::from(r"C:\ObsidianVault")
        );
        // 词法解析：`..` 只是消掉前一段，不会"跳过"盘符根
        assert_eq!(
            normalize(Path::new(r"C:\ObsidianVault\Obsidian-SettingSync\..")),
            PathBuf::from(r"C:\ObsidianVault")
        );
        assert_eq!(
            absolutize(Path::new(".."), Path::new(r"C:\ObsidianVault\Obsidian-SettingSync")),
            PathBuf::from(r"C:\ObsidianVault")
        );
        assert_eq!(
            absolutize(Path::new("Obsidian-AI"), Path::new(r"C:\ObsidianVault")),
            PathBuf::from(r"C:\ObsidianVault\Obsidian-AI")
        );
        // 已经到根之后再 `..` 不应越过根
        assert_eq!(
            normalize(Path::new(r"C:\..\..\ObsidianVault")),
            PathBuf::from(r"C:\ObsidianVault")
        );
    }

    #[test]
    fn loads_real_config_shape() {
        let text = include_str!("../sync.toml");
        let doc = crate::toml::parse(text).expect("sync.toml 应能解析");
        assert_eq!(doc.get("general", "root").unwrap().as_str().unwrap(), "..");
        assert_eq!(doc.get("general", "shared_dir").unwrap().as_str().unwrap(), "Obsidian-Config");
        assert_eq!(doc.get("vaults", "AI").unwrap().as_table().unwrap().get("path").unwrap().as_str().unwrap(), "Obsidian-AI");
        assert_eq!(doc.get("links", "dirs").unwrap().as_array().unwrap().len(), 4);
        assert_eq!(doc.get("links", "files").unwrap().as_array().unwrap().len(), 16);
        assert_eq!(
            doc.get("vaults", "GameDev")
                .unwrap()
                .as_table()
                .unwrap()
                .get("path")
                .unwrap()
                .as_str()
                .unwrap(),
            "Obsidian-GameDev"
        );
    }
}





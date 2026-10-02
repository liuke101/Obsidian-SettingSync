//! 配置模型：从 sync.toml 读取"有哪些库、共享什么、怎么链接"。

use crate::toml::{parse, Doc, Value};
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};
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

#[cfg(test)]
mod link_lifecycle_tests {
    use super::*;
    use crate::{link_vault, verify_into, Output, Report};
    use std::fs;
    use std::path::PathBuf;

    /// 搭一个与真实布局同构的临时环境：配置 + 共享母本 + 一个空库。
    fn setup(tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "obsidian-sync-test-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let shared = root.join("Shared");
        let vault = root.join("V1");
        fs::create_dir_all(shared.join(".obsidian/plugins")).unwrap();
        fs::create_dir_all(shared.join("zip/Templates")).unwrap();
        fs::create_dir_all(vault.join(".obsidian")).unwrap();
        fs::write(shared.join(".obsidian/app.json"), b"{\"shared\":true}\n").unwrap();
        fs::write(shared.join(".obsidian/hotkeys.json"), b"{}\n").unwrap();
        fs::write(shared.join("zip/Templates/t.md"), b"# t\n").unwrap();

        let config_path = root.join("sync.toml");
        let text = format!(
            "[general]\nroot = \"{r}\"\nshared_dir = \"{r}/Shared\"\nbackup_dir = \"{r}/_backup\"\n\n\
             [links]\ndirs = [\".obsidian/plugins\", \"zip\"]\nfiles = [\".obsidian/app.json\", \".obsidian/hotkeys.json\"]\n\n\
             [vaults]\n\"V1\" = {{ path = \"{r}/V1\" }}\n",
            r = root.to_string_lossy().replace('\\', "/")
        );
        fs::write(&config_path, text).unwrap();

        let cfg = load(&config_path, None).expect("配置应能加载");
        (root, shared, vault, cfg.config_path)
    }

    fn opts(resolve: bool) -> crate::Options {
        let mut o = crate::parse_args(&["link".to_string()]).unwrap();
        o.resolve = resolve;
        o
    }

    fn cleanup(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }

    /// 全流程：空库 → 建链接 → 校验通过。
    #[test]
    fn links_an_empty_vault_then_passes_verification() {
        let (root, _shared, vault, cfg_path) = setup("empty");
        let cfg = load(&cfg_path, None).unwrap();
        let v = cfg.vault_by_name("V1").unwrap().clone();

        let mut rep = Report::default();
        link_vault(&cfg, &v, &opts(false), &Output::silent(), &mut rep).unwrap();
        assert_eq!(rep.failures(), 0, "空库建链不应有失败");
        assert_eq!(rep.count(crate::Fate::Create), 4, "应建立 4 条链接");
        assert!(vault.join(".obsidian/app.json").is_file());

        let mut check = Report::default();
        verify_into(&cfg, &[&v], &mut check, false);
        assert_eq!(check.failures(), 0, "校验应无失败");
        assert_eq!(check.count(crate::Fate::Conflict), 0, "校验不应报冲突");

        cleanup(&root);
    }

    /// 幂等：再跑一次不应产生任何改动。
    #[test]
    fn linking_twice_is_idempotent() {
        let (root, _shared, vault, cfg_path) = setup("idem");
        let cfg = load(&cfg_path, None).unwrap();
        let v = cfg.vault_by_name("V1").unwrap().clone();
        let mut first = Report::default();
        link_vault(&cfg, &v, &opts(false), &Output::silent(), &mut first).unwrap();

        let mut second = Report::default();
        link_vault(&cfg, &v, &opts(false), &Output::silent(), &mut second).unwrap();
        assert_eq!(second.count(crate::Fate::Keep), 4, "第二次应全部保持");
        assert_eq!(second.count(crate::Fate::Create), 0);

        cleanup(&root);
    }

    /// 冲突 → 报告为冲突而非失败 → --resolve 备份并改为链接 → 校验通过。
    /// 这条覆盖的正是"新库被 Obsidian 打开后生成默认设置"的真实场景。
    #[test]
    fn reports_conflict_then_resolves_it_with_backup() {
        let (root, _shared, vault, cfg_path) = setup("conflict");
        // 模拟 Obsidian 生成的默认设置：内容与共享母本不同
        fs::write(vault.join(".obsidian/app.json"), b"{\"default\":true}\n").unwrap();

        let cfg = load(&cfg_path, None).unwrap();
        let v = cfg.vault_by_name("V1").unwrap().clone();

        // 第一次：不解析，应报 1 项冲突、0 失败，且文件保持原样
        let mut rep = Report::default();
        link_vault(&cfg, &v, &opts(false), &Output::silent(), &mut rep).unwrap();
        assert_eq!(rep.count(crate::Fate::Conflict), 1, "应报 1 项冲突");
        assert_eq!(rep.failures(), 0, "冲突不算失败");
        assert!(!crate::same_target(&vault.join(".obsidian/app.json"), &cfg.shared_dir.join(".obsidian/app.json")));
        assert_eq!(
            fs::read_to_string(vault.join(".obsidian/app.json")).unwrap(),
            "{\"default\":true}\n",
            "未解析时不得改动冲突文件"
        );

        // 校验也应把它归为冲突而不是失败
        let mut check = Report::default();
        verify_into(&cfg, &[&v], &mut check, false);
        assert_eq!(check.count(crate::Fate::Conflict), 1);
        assert_eq!(check.failures(), 0, "仅冲突时校验失败数应为 0");

        // 第二次：--resolve，应备份并建立链接
        let mut rep2 = Report::default();
        link_vault(&cfg, &v, &opts(true), &Output::silent(), &mut rep2).unwrap();
        assert_eq!(rep2.failures(), 0, "解析冲突不应失败");
        assert_eq!(rep2.count(crate::Fate::Create), 1);
        assert!(
            crate::same_target(
                &vault.join(".obsidian/app.json"),
                &cfg.shared_dir.join(".obsidian/app.json")
            ),
            "解析后应指向共享母本"
        );

        // 备份必须存在，且内容是被替换掉的那份
        let mut found = false;
        for entry in fs::read_dir(&cfg.backup_dir).unwrap() {
            let dir = entry.unwrap().path();
            for f in fs::read_dir(&dir).unwrap() {
                let p = f.unwrap().path();
                if p.file_name().unwrap().to_string_lossy().contains("app.json") {
                    assert_eq!(fs::read_to_string(&p).unwrap(), "{\"default\":true}\n");
                    found = true;
                }
            }
        }
        assert!(found, "被替换的文件必须能在备份目录里找到");

        // 收尾校验
        let mut check2 = Report::default();
        verify_into(&cfg, &[&v], &mut check2, false);
        assert_eq!(check2.failures(), 0);
        assert_eq!(check2.count(crate::Fate::Conflict), 0);

        cleanup(&root);
    }

    #[test]
    fn same_file_content_compares_bytes() {
        let dir = std::env::temp_dir().join(format!("obsidian-sync-cmp-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        let c = dir.join("c.txt");
        fs::write(&a, b"hello").unwrap();
        fs::write(&b, b"hello").unwrap();
        fs::write(&c, b"world").unwrap();
        assert!(crate::same_file_content(&a, &b));
        assert!(!crate::same_file_content(&a, &c));
        assert!(!crate::same_file_content(&a, &dir.join("missing.txt")));
        let _ = fs::remove_dir_all(&dir);
    }
}






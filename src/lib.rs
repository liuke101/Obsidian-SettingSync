//! obsidian-sync —— Obsidian 多仓库配置同步工具（符号链接方式）
//!
//! 本库是 CLI 与 GUI 共用的引擎，保证两条入口行为完全一致：
//!   * 声明式配置：sync.toml 描述有哪些库、共享什么；新增库只需一条命令
//!   * 按需提权：能创建符号链接就直接做，不能则目录自动回退为联接（junction）
//!   * 幂等：重复执行只修复不一致的项，不重建正确的链接
//!   * 可验证：link 后自动校验；status 随时给出每个库的健康快照
//!   * 无损：遇到真实的本地文件先备份再链接，绝不直接删除

pub mod config;
pub mod gui;
pub mod toml;
use config::{normalize, Config, EntryKind, VaultSpec};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    Keep,
    Create,
    Recreate,
    Remove,
    Skip,
    Fail,
}

impl Fate {
    pub fn tag(self) -> &'static str {
        match self {
            Fate::Keep => "OK",
            Fate::Create => "建立",
            Fate::Recreate => "重建",
            Fate::Remove => "移除",
            Fate::Skip => "跳过",
            Fate::Fail => "失败",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Record {
    vault: String,
    rel: String,
    fate: Fate,
    method: String,
    detail: String,
}

#[derive(Default)]
pub struct Report {
    pub records: Vec<Record>,
}

impl Report {
    fn push(&mut self, r: Record) {
        self.records.push(r);
    }
    fn count(&self, fate: Fate) -> usize {
        self.records.iter().filter(|r| r.fate == fate).count()
    }
    fn failures(&self) -> usize {
        self.count(Fate::Fail)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStrategy {
    Symlink,
    Junction,
    Hardlink,
}

impl LinkStrategy {
    pub fn label(self) -> &'static str {
        match self {
            LinkStrategy::Symlink => "符号链接",
            LinkStrategy::Junction => "目录联接",
            LinkStrategy::Hardlink => "硬链接",
        }
    }
}

#[derive(Debug)]
pub enum Existing {
    /// 链接指向 target，可用
    Link { method: LinkStrategy },
    /// 是不可用的链接（断链 / 指错地方）
    BrokenLink { current: Option<PathBuf> },
    /// 真正的本地文件或目录
    Real,
    Missing,
}

pub struct Output {
    verbose: bool,
    /// 简报模式：只打印汇总，不逐项输出（GUI 调用引擎时用）
    brief: bool,
}

impl Output {
    pub fn new(verbose: bool) -> Self {
        Output { verbose, brief: false }
    }
    /// 完全静默（GUI 用，结果经由 JSON 返回前端）
    pub fn silent() -> Self {
        Output { verbose: false, brief: true }
    }
    pub fn say(&self, s: &str) {
        if !self.brief {
            println!("{s}");
        }
    }
    pub fn info(&self, s: &str) {
        if self.verbose && !self.brief {
            println!("  {s}");
        }
    }
}

pub struct Options {
    command: String,
    positional: Vec<String>,
    config_path: Option<PathBuf>,
    root: Option<PathBuf>,
    dry_run: bool,
    force: bool,
    no_verify: bool,
    verbose: bool,
    yes: bool,
    /// gui 子命令专用
    port: u16,
    no_browser: bool,
}

pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        command: String::new(),
        positional: Vec::new(),
        config_path: None,
        root: None,
        dry_run: false,
        force: false,
        no_verify: false,
        verbose: false,
        yes: false,
        port: 0,
        no_browser: false,
    };
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "-h" | "--help" => {
                opts.command = "help".into();
                return Ok(opts);
            }
            "-V" | "--version" => {
                opts.command = "version".into();
                return Ok(opts);
            }
            "-c" | "--config" => {
                i += 1;
                let v = args.get(i).ok_or("--config 后面缺少路径")?;
                opts.config_path = Some(PathBuf::from(v));
            }
            "-r" | "--root" => {
                i += 1;
                let v = args.get(i).ok_or("--root 后面缺少路径")?;
                opts.root = Some(PathBuf::from(v));
            }
            "-p" | "--port" => {
                i += 1;
                let v = args.get(i).ok_or("--port 后面缺少端口号")?;
                let p = v
                    .parse::<u16>()
                    .map_err(|_| format!("端口号无效：{v}"))?;
                opts.port = if p == 0 { 7411 } else { p };
            }
            "--no-browser" => opts.no_browser = true,
            "-n" | "--dry-run" => opts.dry_run = true,
            "-f" | "--force" => opts.force = true,
            "--no-verify" => opts.no_verify = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-y" | "--yes" => opts.yes = true,
            other if other.starts_with('-') => {
                return Err(format!("未知选项：{other}（用 --help 查看用法）"))
            }
            other => {
                if opts.command.is_empty() {
                    opts.command = other.to_string();
                } else {
                    opts.positional.push(other.to_string());
                }
            }
        }
        i += 1;
    }
    if opts.command.is_empty() {
        opts.command = "help".into();
    }
    Ok(opts)
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let opts = parse_args(args)?;
    let out = Output::new(opts.verbose);

    match opts.command.as_str() {
        "help" => {
            print_help();
            Ok(ExitCode::SUCCESS)
        }
        "version" => {
            println!("obsidian-sync {VERSION}");
            Ok(ExitCode::SUCCESS)
        }
        "doctor" => cmd_doctor(&opts, &out),
        "new" => cmd_new(&opts, &out),
        "gui" => gui::serve(gui::GuiOptions {
            port: opts.port,
            open_browser: !opts.no_browser,
            config_path: opts.config_path.clone(),
            root: opts.root.clone(),
        })
        .map(|_| ExitCode::SUCCESS),
        other => {
            let cfg = load_config(&opts)?;
            match other {
                "list" => cmd_list(&cfg, &out),
                "link" => cmd_link(&cfg, &opts, &out),
                "unlink" => cmd_unlink(&cfg, &opts, &out),
                "check" => cmd_check(&cfg, &opts, &out),
                _ => Err(format!("未知命令：{other}（用 --help 查看用法）")),
            }
        }
    }
}

pub fn load_config(opts: &Options) -> Result<Config, String> {
    let path = match &opts.config_path {
        Some(p) => p.clone(),
        None => discover_config()?,
    };
    config::load(&path, opts.root.as_deref())
}

/// 从当前目录向上查找 sync.toml。
pub fn discover_config() -> Result<PathBuf, String> {
    let mut dir = std::env::current_dir().map_err(|e| format!("无法获取当前目录：{e}"))?;
    for _ in 0..8 {
        let candidate = dir.join("sync.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
        let nested = dir.join("Obsidian-SettingSync").join("sync.toml");
        if nested.is_file() {
            return Ok(nested);
        }
        if !dir.pop() {
            break;
        }
    }
    Err("找不到 sync.toml。请在仓库集合根目录执行，或用 --config 指定路径。".into())
}

pub fn print_help() {
    println!(
        r#"obsidian-sync {VERSION} —— Obsidian 多仓库配置同步工具

用法：
  obsidian-sync <命令> [选项]

命令：
  gui                   启动图形界面（浏览器打开，本机回环地址）
  new <路径|名称> [--name 名称] [--profile 配置档]
                        把一个仓库纳入同步并立即建立链接（最常用）
  link  [库名...]       建立/修复共享软连接（省略库名 = 全部）
  check [库名...]       只体检不改动：报告断链、错链、缺失、指向错误
  unlink [库名...]      移除共享软连接（真实文件与本地文件不动）
  list                  列出已配置的库及其共享清单
  doctor                检查环境：权限、共享母本、配置一致性
  help / version

选项：
  -c, --config <文件>   指定 sync.toml（默认从当前目录向上查找）
  -r, --root <目录>     覆盖配置里的 root（仓库集合根目录）
  -p, --port <端口>     gui 监听端口（默认 7411，被占用时自动往后找）
      --no-browser      gui 启动后不自动打开浏览器
  -n, --dry-run         只显示将要做什么，不做任何改动
  -f, --force           覆盖同名真实文件/目录前先备份（默认遇到就跳过）
  -y, --yes             不交互确认
      --no-verify       跳过链接后的校验
  -v, --verbose         输出每一项细节

退出码：0 成功；1 存在失败项；2 用法或配置错误。
"#
    );
}

// ---------------------------------------------------------------- 链接引擎

#[cfg(windows)]
fn attributes_of(p: &Path) -> Option<u32> {
    use std::os::windows::ffi::OsStrExt;
    const INVALID: u32 = 0xFFFF_FFFF;
    let wide: Vec<u16> = p
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let r = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if r == INVALID {
        None
    } else {
        Some(r)
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetFileAttributesW(lp_file_name: *const u16) -> u32;
}

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

#[cfg(windows)]
fn is_reparse_point(p: &Path) -> bool {
    attributes_of(p)
        .map(|a| a & FILE_ATTRIBUTE_REPARSE_POINT != 0)
        .unwrap_or(false)
}

/// 读取链接目标。符号链接与目录联接都会返回 Some；真实文件返回 None。
pub fn read_link_target(p: &Path) -> Option<PathBuf> {
    fs::read_link(p).ok().map(|t| config::strip_verbatim(&t))
}

/// 判断两个路径是否指向同一对象。
/// Windows 下链接目标可能是 `\\?\C:\...` 形式，先去掉该前缀再逐段比较。
pub fn same_target(link_path: &Path, target: &Path) -> bool {
    match read_link_target(link_path) {
        Some(current) => normalize(&config::strip_verbatim(&current)) == normalize(target),
        None => false,
    }
}

/// 链接是否可用：`metadata` 会跟随链接，成功即表示目标真实存在。
/// （空目录也会返回 Ok，避免把空目录误判为断链。）
pub fn link_resolves(p: &Path) -> bool {
    fs::metadata(p).is_ok()
}

/// Windows 上取文件的唯一标识（卷序列号 + 文件索引），用于识别硬链接。
#[cfg(windows)]
fn same_file_id(a: &Path, b: &Path) -> bool {
    match (file_id(a), file_id(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

#[cfg(not(windows))]
fn same_file_id(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.ino() == y.ino() && x.dev() == y.dev(),
        _ => false,
    }
}

#[cfg(windows)]
fn file_id(p: &Path) -> Option<(u32, u32, u32)> {
    use std::os::windows::ffi::OsStrExt;
    const GENERIC_READ: u32 = 0x8000_0000;
    const FILE_SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const INVALID_HANDLE_VALUE: isize = -1;

    let wide: Vec<u16> = p
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_ALL,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle as isize == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut info: ByHandleFileInformation = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        None
    } else {
        Some((
            info.volume_serial_number,
            info.file_index_high,
            info.file_index_low,
        ))
    }
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FileTime {
    low: u32,
    high: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ByHandleFileInformation {
    file_attributes: u32,
    creation_time: FileTime,
    last_access_time: FileTime,
    last_write_time: FileTime,
    volume_serial_number: u32,
    file_size_high: u32,
    file_size_low: u32,
    number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(
        lp_file_name: *const u16,
        dw_desired_access: u32,
        dw_share_mode: u32,
        lp_security_attributes: *const std::ffi::c_void,
        dw_creation_disposition: u32,
        dw_flags_and_attributes: u32,
        h_template_file: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn GetFileInformationByHandle(
        h_file: *mut std::ffi::c_void,
        lp_file_information: *mut ByHandleFileInformation,
    ) -> i32;
    fn CloseHandle(h_object: *mut std::ffi::c_void) -> i32;
}

pub fn inspect(link_path: &Path, target: &Path, kind: EntryKind) -> Existing {
    let meta = match fs::symlink_metadata(link_path) {
        Ok(m) => m,
        Err(_) => return Existing::Missing,
    };

    #[cfg(windows)]
    let reparse = is_reparse_point(link_path);
    #[cfg(not(windows))]
    let reparse = meta.file_type().is_symlink();

    if meta.file_type().is_symlink() || reparse {
        let current = read_link_target(link_path);
        let points_right = same_target(link_path, target);
        let resolves = link_resolves(link_path);
        if points_right && resolves {
            let method = if current.is_some() {
                LinkStrategy::Symlink
            } else {
                LinkStrategy::Junction
            };
            return Existing::Link { method };
        }
        return Existing::BrokenLink { current };
    }

    // 非重解析点：可能是硬链接（两个路径共享同一份数据）
    if kind != EntryKind::Dir && target.exists() && same_file_id(link_path, target) {
        return Existing::Link { method: LinkStrategy::Hardlink };
    }

    Existing::Real
}

pub fn create_link(link_path: &Path, target: &Path, kind: EntryKind) -> Result<LinkStrategy, String> {
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建上级目录失败：{e}"))?;
    }

    let symlink_result = match kind {
        EntryKind::Dir => std::os::windows::fs::symlink_dir(target, link_path),
        _ => std::os::windows::fs::symlink_file(target, link_path),
    };
    match symlink_result {
        Ok(()) => return Ok(LinkStrategy::Symlink),
        Err(e) => {
            let symlink_err = e.to_string();
            if kind == EntryKind::Dir {
                // 目录回退：联接（junction）不需要管理员权限
                match junction::create(target, link_path) {
                    Ok(()) => return Ok(LinkStrategy::Junction),
                    Err(je) => {
                        return Err(format!(
                            "符号链接失败（{symlink_err}），目录联接回退也失败（{je}）。\n\
                             \x20   解决：以管理员身份运行本工具，或在 Windows「设置 → 系统 → 开发者选项」中开启开发者模式。"
                        ))
                    }
                }
            }
            Err(format!(
                "创建符号链接失败：{symlink_err}\n\
                 \x20   文件无法回退为硬链接（硬链接与共享母本共用同一份数据，会带来静默分叉风险）。\n\
                 \x20   解决：以管理员身份运行本工具，或在 Windows「设置 → 系统 → 开发者选项」中开启开发者模式。"
            ))
        }
    }
}

/// 创建目录联接：通过 cmd 的 mklink /J 实现，避免引入额外依赖。
mod junction {
    use std::path::Path;
    use std::process::Command;

    pub fn create(target: &Path, link: &Path) -> Result<(), String> {
        if link.exists() {
            remove_link(link)?;
        }
        let out = Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .output()
            .map_err(|e| format!("调用 mklink 失败：{e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        Err(format!("{}{}", stdout.trim(), stderr.trim()))
    }

    /// 删掉一个链接本体，绝不跟随进入目标。
    pub fn remove_link(link: &Path) -> Result<(), String> {
        let out = Command::new("cmd")
            .arg("/C")
            .arg("rmdir")
            .arg(link)
            .output()
            .map_err(|e| format!("调用 rmdir 失败：{e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

/// 删除链接本体（文件链接用 remove_file，目录链接用 rmdir，都不进入目标）。
pub fn remove_link(link_path: &Path, method: LinkStrategy) -> Result<(), String> {
    match method {
        LinkStrategy::Hardlink => Err("硬链接不自动删除（删它会连带共享母本的数据）".into()),
        LinkStrategy::Junction => junction::remove_link(link_path),
        LinkStrategy::Symlink => {
            let is_dir = link_path.is_dir();
            if is_dir {
                junction::remove_link(link_path)
            } else {
                fs::remove_file(link_path).map_err(|e| format!("删除链接失败：{e}"))
            }
        }
    }
}

// ---------------------------------------------------------------- 报告输出

pub fn print_records(rep: &Report, verbose: bool) {
    let mut by_vault: Vec<(&str, Vec<&Record>)> = Vec::new();
    for r in &rep.records {
        match by_vault.iter_mut().find(|(v, _)| *v == r.vault) {
            Some((_, list)) => list.push(r),
            None => by_vault.push((r.vault.as_str(), vec![r])),
        }
    }
    for (vault, list) in by_vault {
        println!("\n[{vault}]");
        for r in &list {
            let show = verbose
                || matches!(
                    r.fate,
                    Fate::Create | Fate::Recreate | Fate::Remove | Fate::Fail | Fate::Skip
                );
            if !show {
                continue;
            }
            let method = if r.method.is_empty() {
                String::new()
            } else {
                format!("（{}）", r.method)
            };
            let detail = if r.detail.is_empty() {
                String::new()
            } else {
                format!("  {}", r.detail)
            };
            println!("  {:<4} {}{}{}", r.fate.tag(), r.rel, method, detail);
        }
        let ok = list.iter().filter(|r| r.fate == Fate::Keep).count();
        if !verbose && ok > 0 {
            println!("  OK   {ok} 项保持正确");
        }
    }
}

pub fn summarize(rep: &Report) -> ExitCode {
    println!(
        "\n小计：建立 {}，重建 {}，移除 {}，保持 {}，跳过 {}，失败 {}",
        rep.count(Fate::Create),
        rep.count(Fate::Recreate),
        rep.count(Fate::Remove),
        rep.count(Fate::Keep),
        rep.count(Fate::Skip),
        rep.failures()
    );
    if rep.failures() > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

// ---------------------------------------------------------------- 命令实现

pub fn cmd_list(cfg: &Config, out: &Output) -> Result<ExitCode, String> {
    out.say(&format!("仓库集合根目录：{}", cfg.root.display()));
    out.say(&format!("共享内容母本：  {}", cfg.shared_dir.display()));
    out.say(&format!("配置文件：      {}", cfg.config_path.display()));
    out.say(&format!("\n默认共享规则：目录 {} 个，文件 {} 个，其他 {} 个",
        cfg.defaults.dirs.len(), cfg.defaults.files.len(), cfg.defaults.items.len()));
    out.say(&format!("\n已配置 {} 个库：", cfg.vaults.len()));
    for v in &cfg.vaults {
        let set = cfg.entries_for(v);
        let exists = if v.path.is_dir() { "" } else { "  ⚠ 目录不存在" };
        let profile = v
            .profile
            .as_ref()
            .map(|p| format!("  配置档={p}"))
            .unwrap_or_default();
        out.say(&format!(
            "  {:<22} {:<44} 共享 {} 项{}{}",
            v.name,
            v.path.display(),
            set.entries().len(),
            profile,
            exists
        ));
    }
    Ok(ExitCode::SUCCESS)
}

/// `new`：把一个仓库纳入同步并立即链接。
pub fn cmd_new(opts: &Options, out: &Output) -> Result<ExitCode, String> {
    let arg = opts
        .positional
        .first()
        .ok_or("用法：obsidian-sync new <路径|名称> [--name 名称] [--profile 配置档]")?
        .clone();

    // --name / --profile 从剩余位置参数里以 key=value 形式接收，保持参数解析简单
    let mut name: Option<String> = None;
    let mut profile: Option<String> = None;
    for extra in opts.positional.iter().skip(1) {
        if let Some(v) = extra.strip_prefix("--name=") {
            name = Some(v.to_string());
        } else if let Some(v) = extra.strip_prefix("--profile=") {
            profile = Some(v.to_string());
        }
    }

    let cfg = load_config(opts)?;
    let vault_path = {
        let p = Path::new(&arg);
        if p.is_absolute() {
            normalize(p)
        } else if p.is_dir() {
            normalize(&std::env::current_dir().unwrap_or_default().join(p))
        } else {
            normalize(&cfg.root.join(p))
        }
    };
    if !vault_path.is_dir() {
        return Err(format!("目录不存在：{}", vault_path.display()));
    }

    let vault_name = name.unwrap_or_else(|| {
        vault_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "vault".into())
    });

    // 写回 sync.toml
    let rel = relative_to(&cfg.root, &vault_path);
    let mut updated = fs::read_to_string(&cfg.config_path).map_err(|e| e.to_string())?;
    if !updated.contains("[vaults]") {
        updated.push_str("\n[vaults]\n");
    }
    let line = match &profile {
        Some(p) => format!("\"{vault_name}\" = {{ path = \"{rel}\", profile = \"{p}\" }}\n"),
        None => format!("\"{vault_name}\" = {{ path = \"{rel}\" }}\n"),
    };
    if cfg.vault_by_name(&vault_name).is_some() {
        out.say(&format!("库 {vault_name} 已在配置中，跳过写入，直接执行链接。"));
    } else {
        if opts.dry_run {
            out.say(&format!("[dry-run] 将向 {} 追加：{line}", cfg.config_path.display()));
        } else {
            let mut f = fs::OpenOptions::new()
                .append(true)
                .open(&cfg.config_path)
                .map_err(|e| format!("写入配置失败：{e}"))?;
            f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
            out.say(&format!("已写入配置：{}  {line}", cfg.config_path.display()));
        }
    }

    // 重新加载后执行链接
    let cfg = load_config(opts)?;
    let target = cfg
        .vault_by_name(&vault_name)
        .ok_or("写入配置后仍找不到该库，请检查 sync.toml")?
        .clone();
    let mut rep = Report::default();
    link_vault(&cfg, &target, opts, out, &mut rep)?;
    print_records(&rep, opts.verbose);
    Ok(summarize(&rep))
}

pub fn relative_to(base: &Path, p: &Path) -> String {
    p.strip_prefix(base)
        .map(|s| s.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| p.to_string_lossy().replace('\\', "/"))
}

pub fn select_vaults<'a>(cfg: &'a Config, names: &[String]) -> Result<Vec<&'a VaultSpec>, String> {
    if names.is_empty() {
        if cfg.vaults.is_empty() {
            return Err("sync.toml 里还没有任何库，先执行 obsidian-sync new <路径>".into());
        }
        return Ok(cfg.vaults.iter().collect());
    }
    let mut out = Vec::new();
    for n in names {
        let v = cfg
            .vault_by_name(n)
            .ok_or_else(|| format!("配置里没有名为 {n} 的库"))?;
        out.push(v);
    }
    Ok(out)
}

pub fn cmd_link(cfg: &Config, opts: &Options, out: &Output) -> Result<ExitCode, String> {
    let vaults = select_vaults(cfg, &opts.positional)?;
    check_shared_dir(cfg, out)?;

    let mut rep = Report::default();
    for v in &vaults {
        link_vault(cfg, v, opts, out, &mut rep)?;
    }

    if cfg.verify && !opts.no_verify && !opts.dry_run {
        println!("\n—— 校验 ——");
        verify_into(cfg, &vaults, &mut rep, opts.verbose);
    }
    print_records(&rep, opts.verbose);
    Ok(summarize(&rep))
}

pub fn check_shared_dir(cfg: &Config, out: &Output) -> Result<(), String> {
    if !cfg.shared_dir.is_dir() {
        return Err(format!(
            "共享内容母本不存在：{}\n  请确认 sync.toml 里的 shared_dir。",
            cfg.shared_dir.display()
        ));
    }
    let obsidian = cfg.shared_dir.join(".obsidian");
    if !obsidian.is_dir() {
        out.info(&format!("提示：{} 下没有 .obsidian 目录", cfg.shared_dir.display()));
    }
    Ok(())
}

pub fn link_vault(
    cfg: &Config,
    vault: &VaultSpec,
    opts: &Options,
    out: &Output,
    rep: &mut Report,
) -> Result<(), String> {
    if !vault.path.is_dir() {
        return Err(format!(
            "库目录不存在：{}（库名 {}）",
            vault.path.display(),
            vault.name
        ));
    }
    out.say(&format!("\n== 处理 {} ==", vault.name));
    out.info(&format!("库目录：{}", vault.path.display()));

    let entries = cfg.entries_for(vault);
    for entry in entries.entries() {
        let kind = entry.kind.resolve(&entry.rel);
        let target = cfg.shared_dir.join(rel_to_native(&entry.rel));
        let link_path = vault.path.join(rel_to_native(&entry.rel));

        if !target.exists() {
            rep.push(Record {
                vault: vault.name.clone(),
                rel: entry.rel.clone(),
                fate: Fate::Skip,
                method: String::new(),
                detail: format!("共享母本里不存在：{}", target.display()),
            });
            continue;
        }

        let existing = inspect(&link_path, &target, kind);
        match existing {
            Existing::Link { method } => {
                rep.push(Record {
                    vault: vault.name.clone(),
                    rel: entry.rel.clone(),
                    fate: Fate::Keep,
                    method: method.label().into(),
                    detail: String::new(),
                });
                continue;
            }
            Existing::Real => {
                if !opts.force {
                    rep.push(Record {
                        vault: vault.name.clone(),
                        rel: entry.rel.clone(),
                        fate: Fate::Skip,
                        method: String::new(),
                        detail: "已存在真实文件/目录，未改动（加 --force 可先备份再链接）".into(),
                    });
                    continue;
                }
                if opts.dry_run {
                    rep.push(Record {
                        vault: vault.name.clone(),
                        rel: entry.rel.clone(),
                        fate: Fate::Create,
                        method: String::new(),
                        detail: "[dry-run] 将先备份再建立链接".into(),
                    });
                    continue;
                }
                if cfg.auto_backup {
                    let backup = backup_path(cfg, &vault.name, &entry.rel)?;
                    move_aside(&link_path, &backup)?;
                    out.info(&format!("已备份到 {}", backup.display()));
                } else {
                    // auto_backup = false：整体挪走而不是删除，避免误删数据
                    let backup = backup_path(cfg, &vault.name, &entry.rel)?;
                    move_aside(&link_path, &backup)?;
                    out.info(&format!(
                        "已把原文件移到 {}（auto_backup=false 时仍不删除，只挪走）",
                        backup.display()
                    ));
                }
            }
            Existing::BrokenLink { current } => {
                if !opts.dry_run {
                    // 断链 / 错链：直接替换，无需备份（里面没有用户数据）
                    let method = match current {
                        Some(_) => LinkStrategy::Symlink,
                        None => LinkStrategy::Junction,
                    };
                    if let Err(e) = remove_link(&link_path, method) {
                        // 联接回退：用 rmdir 再试一次
                        if let Err(e2) = junction::remove_link(&link_path) {
                            rep.push(Record {
                                vault: vault.name.clone(),
                                rel: entry.rel.clone(),
                                fate: Fate::Fail,
                                method: String::new(),
                                detail: format!("清理失效链接失败：{e} / {e2}"),
                            });
                            continue;
                        }
                    }
                }
                let fate = Fate::Recreate;
                if opts.dry_run {
                    rep.push(Record {
                        vault: vault.name.clone(),
                        rel: entry.rel.clone(),
                        fate,
                        method: String::new(),
                        detail: "[dry-run] 将重建失效链接".into(),
                    });
                    continue;
                }
                match create_link(&link_path, &target, kind) {
                    Ok(method) => rep.push(Record {
                        vault: vault.name.clone(),
                        rel: entry.rel.clone(),
                        fate,
                        method: method.label().into(),
                        detail: "原链接失效或指向错误".into(),
                    }),
                    Err(e) => rep.push(Record {
                        vault: vault.name.clone(),
                        rel: entry.rel.clone(),
                        fate: Fate::Fail,
                        method: String::new(),
                        detail: e,
                    }),
                }
                continue;
            }
            Existing::Missing => {}
        }

        if opts.dry_run {
            rep.push(Record {
                vault: vault.name.clone(),
                rel: entry.rel.clone(),
                fate: Fate::Create,
                method: String::new(),
                detail: "[dry-run] 将建立链接".into(),
            });
            continue;
        }

        match create_link(&link_path, &target, kind) {
            Ok(method) => {
                let mut detail = String::new();
                if method == LinkStrategy::Junction {
                    detail = "符号链接失败，已回退为目录联接".into();
                }
                rep.push(Record {
                    vault: vault.name.clone(),
                    rel: entry.rel.clone(),
                    fate: Fate::Create,
                    method: method.label().into(),
                    detail,
                });
            }
            Err(e) => rep.push(Record {
                vault: vault.name.clone(),
                rel: entry.rel.clone(),
                fate: Fate::Fail,
                method: String::new(),
                detail: e,
            }),
        }
    }
    Ok(())
}

pub fn rel_to_native(rel: &str) -> PathBuf {
    PathBuf::from(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
}

pub fn backup_path(cfg: &Config, vault: &str, rel: &str) -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let flat = rel.replace(['/', '\\'], "_");
    let dir = cfg.backup_dir.join(format!("{vault}-{stamp}"));
    fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败：{e}"))?;
    Ok(dir.join(flat))
}

/// 把真实文件/目录整体挪到备份位置（同盘移动是瞬时的，不复制数据）。
pub fn move_aside(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            // 跨卷时回退为复制 + 删除
            copy_recursive(from, to)?;
            if from.is_dir() {
                fs::remove_dir_all(from).map_err(|e| format!("清理原目录失败：{e}"))
            } else {
                fs::remove_file(from).map_err(|e| format!("清理原文件失败：{e}"))
            }
        }
    }
}

fn copy_recursive(from: &Path, to: &Path) -> Result<(), String> {
    if from.is_dir() {
        fs::create_dir_all(to).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(|_| ()).map_err(|e| format!("复制失败：{e}"))
    }
}

/// 把校验结果追加进同一个报告。
pub fn verify_into(cfg: &Config, vaults: &[&VaultSpec], rep: &mut Report, verbose: bool) {
    for vault in vaults {
        let entries = cfg.entries_for(vault);
        for entry in entries.entries() {
            let kind = entry.kind.resolve(&entry.rel);
            let target = cfg.shared_dir.join(rel_to_native(&entry.rel));
            let link_path = vault.path.join(rel_to_native(&entry.rel));
            let (fate, detail) = match inspect(&link_path, &target, kind) {
                Existing::Link { method } => (Fate::Keep, format!("{} 可读", method.label())),
                Existing::BrokenLink { current } => (
                    Fate::Fail,
                    match current {
                        Some(c) => format!("链接指向 {}，应为 {}", c.display(), target.display()),
                        None => "链接无法解析".to_string(),
                    },
                ),
                Existing::Real => (Fate::Fail, "不是链接（是真实文件/目录）".to_string()),
                Existing::Missing => (Fate::Fail, "链接缺失".to_string()),
            };
            if fate == Fate::Keep && !verbose {
                continue;
            }
            rep.push(Record {
                vault: format!("{} · 校验", vault.name),
                rel: entry.rel.clone(),
                fate,
                method: String::new(),
                detail,
            });
        }
    }
}

pub fn cmd_check(cfg: &Config, opts: &Options, out: &Output) -> Result<ExitCode, String> {
    let vaults = select_vaults(cfg, &opts.positional)?;
    out.say(&format!(
        "共享内容母本：{}\n",
        cfg.shared_dir.display()
    ));
    let mut rep = Report::default();
    verify_into(cfg, &vaults, &mut rep, true);
    print_verify(&rep, out);
    let bad = rep.failures();
    if bad == 0 {
        out.say(&format!(
            "\n体检通过：{} 个库共 {} 项链接全部正确。",
            vaults.len(),
            rep.records.len()
        ));
        Ok(ExitCode::SUCCESS)
    } else {
        out.say(&format!("\n发现 {bad} 项问题，可用 obsidian-sync link 修复。"));
        Ok(ExitCode::from(1))
    }
}

pub fn print_verify(rep: &Report, out: &Output) {
    let mut current = String::new();
    for r in &rep.records {
        if r.vault != current {
            out.say(&format!("\n[{}]", r.vault));
            current = r.vault.clone();
        }
        out.say(&format!("  {:<4} {:<28} {}", r.fate.tag(), r.rel, r.detail));
    }
}

pub fn cmd_unlink(cfg: &Config, opts: &Options, out: &Output) -> Result<ExitCode, String> {
    let vaults = select_vaults(cfg, &opts.positional)?;
    out.say(&format!(
        "将移除 {} 个库中的共享链接（真实文件与本地文件不会被删除）",
        vaults.len()
    ));
    if !confirm(opts, out)? {
        out.say("已取消。");
        return Ok(ExitCode::SUCCESS);
    }

    let mut rep = Report::default();
    for vault in &vaults {
        for entry in cfg.entries_for(vault).entries() {
            let kind = entry.kind.resolve(&entry.rel);
            let target = cfg.shared_dir.join(rel_to_native(&entry.rel));
            let link_path = vault.path.join(rel_to_native(&entry.rel));
            let (fate, method, detail) = match inspect(&link_path, &target, kind) {
                Existing::Link { method } => {
                    if opts.dry_run {
                        (Fate::Remove, String::new(), "[dry-run] 将移除链接".to_string())
                    } else {
                        match remove_link(&link_path, method) {
                            Ok(()) => (
                                Fate::Recreate,
                                method.label().to_string(),
                                "已移除".to_string(),
                            ),
                            Err(e) => (Fate::Fail, String::new(), e),
                        }
                    }
                }
                Existing::BrokenLink { .. } => {
                    if opts.dry_run {
                        (Fate::Remove, String::new(), "[dry-run] 将移除失效链接".to_string())
                    } else {
                        match junction::remove_link(&link_path) {
                            Ok(()) => (Fate::Remove, "失效链接".into(), "已移除".to_string()),
                            Err(e) => (Fate::Fail, String::new(), e),
                        }
                    }
                }
                Existing::Real => (
                    Fate::Skip,
                    String::new(),
                    "是真实文件/目录，未改动".to_string(),
                ),
                Existing::Missing => (
                    Fate::Skip,
                    String::new(),
                    "本来就不存在".to_string(),
                ),
            };
            rep.push(Record {
                vault: vault.name.clone(),
                rel: entry.rel.clone(),
                fate,
                method,
                detail,
            });
        }
    }
    print_records(&rep, opts.verbose);
    Ok(summarize(&rep))
}

pub fn confirm(opts: &Options, out: &Output) -> Result<bool, String> {
    if opts.yes || opts.dry_run {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        out.say("（非交互环境，默认继续；加 -y 可显式确认）");
        return Ok(true);
    }
    print!("确认执行？[y/N] ");
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes" | "YES"))
}

// ---------------------------------------------------------------- doctor

pub fn cmd_doctor(opts: &Options, out: &Output) -> Result<ExitCode, String> {
    let mut problems = 0usize;
    out.say("== 环境检查 ==");

    // 1) 能否创建符号链接
    let probe_dir = std::env::temp_dir().join(format!("obsidian-sync-probe-{}", std::process::id()));
    fs::create_dir_all(&probe_dir).map_err(|e| e.to_string())?;
    let probe_target = probe_dir.join("target.txt");
    fs::write(&probe_target, b"probe").map_err(|e| e.to_string())?;
    let probe_link = probe_dir.join("link.txt");
    let symlink_ok = std::os::windows::fs::symlink_file(&probe_target, &probe_link).is_ok();
    if symlink_ok {
        out.say("  OK   可以创建符号链接");
    } else {
        out.say("  ⚠    无法创建符号链接");
        out.say("       目录项会自动回退为「目录联接」，文件项会失败。");
        out.say("       解决：以管理员身份运行，或在「设置 → 系统 → 开发者选项」开启开发者模式。");
        problems += 1;
    }
    fs::remove_file(&probe_link).ok();
    fs::remove_dir_all(&probe_dir).ok();

    // 2) 配置
    let cfg = match load_config(opts) {
        Ok(c) => {
            out.say(&format!("  OK   配置文件：{}", c.config_path.display()));
            Some(c)
        }
        Err(e) => {
            out.say(&format!("  ⚠    配置不可用：{e}"));
            problems += 1;
            None
        }
    };

    let Some(cfg) = cfg else {
        out.say(&format!("\n发现 {problems} 个问题。"));
        return Ok(ExitCode::from(1));
    };

    // 3) 共享母本
    if cfg.shared_dir.is_dir() {
        let n = fs::read_dir(&cfg.shared_dir).map(|d| d.count()).unwrap_or(0);
        out.say(&format!(
            "  OK   共享内容母本：{}（{n} 个顶层项）",
            cfg.shared_dir.display()
        ));
    } else {
        out.say(&format!("  ⚠    共享内容母本不存在：{}", cfg.shared_dir.display()));
        problems += 1;
    }

    // 4) 每个库
    out.say("\n== 各库状态 ==");
    for v in &cfg.vaults {
        if !v.path.is_dir() {
            out.say(&format!("  ⚠    {:<22} 目录不存在：{}", v.name, v.path.display()));
            problems += 1;
            continue;
        }
        let entries = cfg.entries_for(v).entries();
        let mut bad = Vec::new();
        for e in &entries {
            let kind = e.kind.resolve(&e.rel);
            let target = cfg.shared_dir.join(rel_to_native(&e.rel));
            let link_path = v.path.join(rel_to_native(&e.rel));
            match inspect(&link_path, &target, kind) {
                Existing::Link { .. } => {}
                Existing::BrokenLink { .. } => bad.push(format!("{}（失效/指错）", e.rel)),
                Existing::Real => bad.push(format!("{}（真实文件）", e.rel)),
                Existing::Missing => bad.push(format!("{}（缺失）", e.rel)),
            }
        }
        if bad.is_empty() {
            out.say(&format!(
                "  OK    {:<22} {} 项链接全部正确",
                v.name,
                entries.len()
            ));
        } else {
            out.say(&format!(
                "  ⚠    {:<22} {}/{} 项有问题：{}",
                v.name,
                bad.len(),
                entries.len(),
                bad.join("、")
            ));
            problems += 1;
        }
    }

    // 5) 配置档引用是否有效
    for v in &cfg.vaults {
        if let Some(p) = &v.profile {
            if !cfg.profiles.contains_key(p) {
                out.say(&format!("  ⚠    {} 引用了不存在的配置档 {p}", v.name));
                problems += 1;
            }
        }
    }

    if problems == 0 {
        out.say("\n环境正常，可以直接执行 obsidian-sync link。");
        Ok(ExitCode::SUCCESS)
    } else {
        out.say(&format!("\n发现 {problems} 个问题，详见上方。"));
        Ok(ExitCode::from(1))
    }
}



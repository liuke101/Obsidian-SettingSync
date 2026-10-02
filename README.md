# Obsidian Setting Sync

Obsidian 多仓库**配置共享**工具：让 N 个仓库共用同一份插件、主题、快捷键与模板，
新增一个仓库只需一条命令。

- 单文件可执行（`bin\obsidian-sync.exe`，约 300 KB），**不需要安装任何运行时**
- 声明式配置：`sync.toml` 描述「有哪些库、共享什么」
- **按需提权**：能建符号链接就直接建；不能则目录自动回退为「目录联接」，不中断
- **幂等**：重复执行只修复不一致的项，不会重建已正确的链接
- **无损**：遇到同名真实文件先整体挪到备份目录，绝不直接删除
- **可体检**：`check` 随时报告断链、错链、缺失、指错目标

---

## 一、为什么是这个形态

| 旧版（已归档到 `legacy/`） | 本版 |
|---|---|
| C# WPF 图形界面，需装 .NET 10 运行时 | Rust 单文件 exe，零依赖 |
| 手动选目标/源路径、手填排除清单 | 读 `sync.toml`，规则写一次到处适用 |
| 删除模式对真实目录是**递归删除** | 永不删除真实文件，只整体挪到备份 |
| 无校验，建完不知道对不对 | `link` 后自动校验，`check` 可随时体检 |
| 新增库要重新点一遍界面 | `obsidian-sync new <路径>` 一条命令 |

选 Rust 的理由：单文件绿色可执行（复制即用）、启动毫秒级、无需运行时，
且能直接操作 Windows 重解析点与目录联接。

---

## 二、构建

```cmd
build.cmd
```

前置条件：Rust 工具链（<https://rustup.rs>）。产物：`bin\obsidian-sync.exe`。

构建脚本会先跑单元测试，测试不过则中止。

---

## 三、日常用法

在仓库集合根目录（或任意子目录）执行，工具会自己向上找到 `sync.toml`：

```cmd
bin\obsidian-sync.exe doctor           :: 先看环境与各库状态
bin\obsidian-sync.exe link             :: 建立/修复全部链接
bin\obsidian-sync.exe check            :: 只体检，不改动
bin\obsidian-sync.exe list             :: 看有哪些库、共享多少项
```

### 新增一个仓库（最常见的操作）

```cmd
bin\obsidian-sync.exe new C:\ObsidianVault\Obsidian-New
```

这一条命令会：

1. 把该库登记进 `sync.toml`（路径自动写成相对 `root` 的形式）
2. 按共享规则建立全部链接
3. 自动校验一遍并打印结果

若该库在 Obsidian 里还没打开过（没有 `.obsidian` 目录），先打开一次让它生成默认配置，
再执行上面的命令；或者直接执行——工具会自动创建缺失的上级目录。

### 其他命令

```cmd
:: 只处理指定库
bin\obsidian-sync.exe link GameDev

:: 预演，不动任何文件
bin\obsidian-sync.exe link --dry-run

:: 某库的 .obsidian 里已有真实配置文件，想用共享版覆盖（先备份再链接）
bin\obsidian-sync.exe link NewVault --force

:: 移除共享链接（真实文件与本地文件不动），换机器或不再共享时用
bin\obsidian-sync.exe unlink -y

:: 指定别的配置文件
bin\obsidian-sync.exe -c D:\other\sync.toml link
```

退出码：`0` 成功；`1` 有失败项（脚本里可直接判断）；`2` 用法或配置错误。

---

## 四、权限：为什么有时需要管理员

Windows 上创建**符号链接**需要「管理员权限」或「开发者模式」二者之一：

| 环境 | 行为 |
|---|---|
| 已开开发者模式 | 全部项直接建符号链接，无需管理员 |
| 管理员身份运行 | 同上 |
| 都不是 | **目录**项自动回退为「目录联接」（junction，无需权限）；**文件**项失败并给出提示 |

开启开发者模式：`设置 → 系统 → 开发者选项 → 开发者模式`。
`doctor` 会直接告诉你当前处于哪种状态。

> 文件项**不会**回退为硬链接：硬链接与共享母本共用同一份数据，一旦断开不会有任何报错，
> 会造成「以为在同步、实际已经分叉」。宁可直接失败。

---

## 五、配置文件 `sync.toml`

```toml
[general]
root = ".."                          # 仓库集合根目录（相对本文件），下面所有相对路径都以它为基准
shared_dir = "Obsidian-Config"       # 共享内容母本
auto_backup = true                   # 覆盖真实文件前自动备份
backup_dir = "_backup"
verify = true                        # 链接后自动校验

[links]
dirs  = [".obsidian/plugins", ".obsidian/themes", ".obsidian/snippets", "zip"]
files = [".obsidian/app.json", ...]  # 16 个共享的设置文件

[profiles.minimal]                   # 可选：给特殊库增减规则
extend = false
dirs = [".obsidian/plugins"]

[vaults]
"AI" = { path = "Obsidian-AI" }
```

要点：

- **只把「各库应该一致」的东西放进共享清单。** 判断标准很简单：
  这个文件的内容在不同库里该不该长得一样？
  - 该一样 → 共享（如插件的启用列表、主题、快捷键、模板目录）
  - 本来就该不同 → 本地文件（如 `workspace.json` 面板布局、`.gitignore` 忽略规则）
- 写错的共享项会被 `check` / `doctor` 直接报出来，不会静默生效。
- 路径一律相对 `root`（**不是**相对 `sync.toml`），`..` 只是普通的上一级。

---

## 六、目录结构

```
ObsidianVault\
├── Obsidian-AI\            ─┐
├── Obsidian-GameDev\        │ 内容库：各自 git，各自发布
├── Obsidian-Misc\           │ 每个库 20 条链接 → Obsidian-Config
├── Obsidian-TechArt\       ─┘
├── Obsidian-Config\        ← 共享内容母本（.obsidian + zip 资源），独立 git 仓库
├── Obsidian-SettingSync\   ← 本仓库：工具源码 + sync.toml
│   ├── bin\obsidian-sync.exe
│   ├── src\                工具源码（Rust）
│   ├── legacy\             旧版 C# WPF 工具（已弃用，仅存档）
│   ├── sync.toml           库清单与共享规则
│   └── build.cmd
└── _backup\                ← 被覆盖的真实文件备份
```

工作方式的完整说明见 [`..\Obsidian-Config\README.md`](../Obsidian-Config/README.md)。

---

## 七、开发

```cmd
cargo test          :: 单元测试（含 sync.toml 形状校验）
cargo build --release
```

源码结构：

| 文件 | 职责 |
|---|---|
| `src/main.rs` | 命令解析、链接引擎、体检与报告 |
| `src/config.rs` | 配置模型与路径解析 |
| `src/toml.rs` | 零依赖的 TOML 子集解析器（带单测） |

刻意**不引入任何第三方 crate**：这样构建不需要网络，二进制也不带供应链风险。

# Obsidian Setting Sync

Obsidian 多仓库**配置共享**工具：让 N 个仓库共用同一份插件、主题、快捷键与模板，
新增一个仓库只需一条命令——命令行与图形界面两种用法，行为完全一致。

- 单文件可执行（`bin\obsidian-sync.exe`，约 350 KB），**不需要安装任何运行时**
- **内置图形界面**：`bin\obsidian-sync.exe gui` 自动打开浏览器控制台
- 声明式配置：`sync.toml` 描述「有哪些库、共享什么」
- **按需提权**：能建符号链接就直接建；不能则目录自动回退为「目录联接」，不中断
- **幂等**：重复执行只修复不一致的项，不会重建已正确的链接
- **无损**：遇到同名真实文件先整体挪到备份目录，绝不直接删除
- **可体检**：`check` 随时报告断链、错链、缺失、指错目标

---

## 一、设计取舍

| 要点 | 做法 |
|---|---|
| 部署 | Rust 单文件 exe（约 350 KB），零运行时依赖，复制即用 |
| 界面 | 内置本机 HTTP 服务 + 单页界面，**不引入任何 GUI 框架**，仍是单文件 |
| 配置 | 读 `sync.toml`，规则写一次到处适用，不靠每次手填 |
| 删除行为 | **永不删除真实文件**，只整体挪到 `_backup`；默认遇到就跳过 |
| 正确性 | `link` 后自动校验，`check` / `doctor` 可随时体检 |
| 新增库 | 界面点一下，或 `obsidian-sync new <路径>`，都会登记并建链 |

选 Rust 的理由：单文件绿色可执行、启动毫秒级、无需运行时，
且能直接操作 Windows 重解析点与目录联接；不使用任何第三方 crate，
所以构建不需要网络，二进制也不带供应链风险。

界面为什么用浏览器而不是原生窗口：原生 GUI 框架会带来数十个依赖、
让二进制膨胀到几 MB 并引入供应链风险；而"本机回环地址 + 单页界面"
保持零依赖、体积不变，交互体验也足够。服务只绑定 `127.0.0.1`，不对外暴露。

---

## 二、构建

```cmd
build.cmd
```

前置条件：Rust 工具链（<https://rustup.rs>）。产物：`bin\obsidian-sync.exe`。

构建脚本会先跑单元测试，测试不过则中止。

---

## 三、图形界面（推荐）

```cmd
bin\obsidian-sync.exe gui
```

浏览器会自动打开 `http://127.0.0.1:7411/`。界面上能做的事：

| 区域 | 功能 |
|---|---|
| 顶部信息 | 配置文件、仓库集合、共享母本的路径；符号链接是否可用 |
| 统计 | 已配置库数、共享链接项总数、完全正常的库数、待修复项数 |
| 工具栏 | 只体检 / 建立与修复 / 移除共享链接 / 重新读取 |
| 库卡片 | 每库状态徽标与 `正常数/总数`，展开看逐项明细（哪条断了、指向哪里） |
| 卡片按钮 | 建立修复此库、体检此库、用 Obsidian 打开 |
| 添加表单 | 填目录（可留空库名、选配置档）→ 添加并建立链接 |
| 操作日志 | 每次动作的逐项结果与汇总 |

细节：

- 工具栏按钮在**勾选了库卡片左侧复选框**时只作用于选中的库，否则作用于全部。
- 「移除共享链接」会二次确认，且只删链接——`workspace.json` 等本地文件与真实文件不动。
- `--port 7500` 可换端口（默认 7411，被占用时自动往后试 20 个）；
  `--no-browser` 只起服务不开浏览器。
- 按 `Ctrl+C` 结束服务。

---

## 四、命令行用法

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

:: 解决"设置冲突"（新库首次被 Obsidian 打开后最常见的问题，见第五节）
bin\obsidian-sync.exe link --resolve

:: 移除共享链接（真实文件与本地文件不动），换机器或不再共享时用
bin\obsidian-sync.exe unlink -y

:: 指定别的配置文件
bin\obsidian-sync.exe -c D:\other\sync.toml link
```

退出码：`0` 成功；`1` 有失败项（脚本里可直接判断）；`2` 用法或配置错误。
**只有冲突、没有链接故障时退出码为 0**——冲突是可解释、可一键解决的状态，不算故障。

---

## 五、设置冲突：新库为什么会有几项链接不上

**现象**：新建的库执行 `link` 后，总有几个文件（通常是 `app.json`、`appearance.json`、
`core-plugins.json`、`graph.json`）报告"冲突"而不是"建立"。

**原因**：新库一旦被 Obsidian 打开一次，Obsidian 就会在里面**生成一份自己的默认设置**。
这些是真实文件，不是链接。工具的原则是**绝不擅自删除真实文件**，因此默认只报告冲突、不动它们。

**解决**：

```cmd
bin\obsidian-sync.exe link --resolve
```

或在界面里点「解决全部冲突」按钮（界面默认就带 `resolve`）。

处理方式是三步，全程不丢数据：

1. 把冲突的真实文件**整体移动**（不是删除、不是复制）到 `_backup\<库名>-<时间戳>\`；
2. 在原位置建立指向共享母本的链接；
3. 顺带告诉你原文件内容**是否与母本一致**——一致说明那只是 Obsidian 生成的默认值，
   不同则说明是 Obsidian 的默认值或本地改动，两样都留在备份里，随时可还原。

> 为什么不做成"自动删除"：删除是不可逆的，而"移入备份"效果一样却可回退。
> 本工具在设计上的一条硬规则是**永不删除用户的真实文件**。

---

## 六、权限：为什么有时需要管理员

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

## 七、配置文件 `sync.toml`

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

## 八、目录结构

```
ObsidianVault\
├── Obsidian-AI\            ─┐
├── Obsidian-GameDev\        │ 内容库：各自 git，各自发布
├── Obsidian-Misc\           │ 每个库 20 条链接 → Obsidian-Config
├── Obsidian-TechArt\       ─┘
├── Obsidian-Config\        ← 共享内容母本（.obsidian + zip 资源），独立 git 仓库
├── Obsidian-SettingSync\   ← 本仓库：工具源码 + sync.toml
│   ├── 启动界面.cmd         双击即启动图形界面
│   ├── bin\obsidian-sync.exe
│   ├── src\                工具源码（Rust）：lib.rs 引擎 / main.rs 命令行 / gui.rs 界面 / ui.html
│   ├── sync.toml           库清单与共享规则
│   └── build.cmd
└── _backup\                ← 被覆盖的真实文件备份
```

工作方式的完整说明见 [`..\Obsidian-Config\README.md`](../Obsidian-Config/README.md)。

---

## 九、开发

```cmd
cargo test          :: 单元测试（含 sync.toml 形状校验）
cargo build --release
```

源码结构：

| 文件 | 职责 |
|---|---|
| `src/lib.rs` | **引擎**：链接判定/建立/移除、体检、报告；CLI 与 GUI 共用 |
| `src/main.rs` | 命令行入口（只做参数转发） |
| `src/gui.rs` | 本机 HTTP 服务、JSON 序列化、状态快照 |
| `src/ui.html` | 内嵌的单页界面（HTML+CSS+JS，无外部资源） |
| `src/config.rs` | 配置模型与路径解析 |
| `src/toml.rs` | 零依赖的 TOML 子集解析器 |

CLI 与 GUI **调用同一套引擎函数**（`link_vault` / `verify_into` / `inspect` 等），
所以两条入口的行为不会分叉；界面上的每个动作都等价于一条 CLI 命令。

刻意**不引入任何第三方 crate**：这样构建不需要网络，二进制也不带供应链风险。




# 旧版工具（已弃用，仅存档）

这里的 `ObsidianSettingSync`（C# WPF）是 2026-10-02 之前的配置同步工具，
现已被仓库根目录的 **`obsidian-sync`（Rust 命令行工具）** 取代。

保留它只是为了留个记录，**请不要再使用**。原因：

1. **删除模式有破坏性**：其 `TryDelete` 对「非软连接的真实目录」会执行
   `DeleteDirectory(recursive: true)`。如果某条链接已经断开、变成了同名真实目录，
   点一次「删除软连接」就会把里面删空。
2. **需要 .NET 10 运行时**，而新工具是单文件绿色可执行。
3. **无校验**：建完之后无法确认每条链接是否真的指向正确目标。
4. 排除清单、目标/源路径全靠每次手填，新增库要重复点一遍界面。

新工具的设计取舍见 [`../README.md`](../README.md)。

## 想恢复使用？

不建议。若确实需要，重新构建方式：

```cmd
dotnet build ObsidianSettingSync\ObsidianSettingSync.csproj
```

构建产物（`bin/`、`obj/`）已被 `.gitignore` 排除，不纳入版本管理。

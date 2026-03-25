# ObsidianSettingSync

用于 Obsidian 多仓库配置同步（符号链接方式）。


注意事项：
**1. 请以管理员模式运行！**
2. 默认创建符号链接

## 当前版本

- 已重构为 C# WPF 图形界面应用。
- 首页提供两个按钮：`创建软连接`、`删除软连接`。
- 点击按钮后进入路径输入界面，可通过按钮选择本地目标/源仓库 `.Obsidian` 文件夹。
- 每一步关键按钮点击都会弹出确认对话框，防止误操作。

## 使用步骤

1. 以管理员身份启动程序。
2. 在首页选择 `创建软连接` 或 `删除软连接`。
3. 在下一页选择：
	- 目标仓库 `.Obsidian` 文件夹路径
	- 源仓库 `.Obsidian` 文件夹路径
4. 点击执行按钮并在弹窗中确认。
5. 在右侧日志区查看每一项处理结果。

![image](https://github.com/liuke101/Obsidian-SettingSync/assets/63388681/9c594586-dbc4-4548-82b5-d0b9a5628544)


支持 `.Obsidian` 路径内的文件夹和 JSON 文件，默认处理列表如下：
![image](https://github.com/liuke101/Obsidian-SettingSync/assets/63388681/6c654af7-b8e0-423e-a7f1-1c8ef3a11441)


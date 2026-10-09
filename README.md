# 桌伴 DeskBuddy

一个本地运行的桌面陪伴应用。通过角色动作和会话通知，感知编码工具的工作进度。

<img src="assets/brand/deskbuddy-logo.png" alt="DeskBuddy" width="160">

## 功能

- 透明置顶窗口、自定义动画角色（可导入或制作，也可随安装包内置）、拖动和托盘菜单。
- 展示会话名称与内容，区分处理中、完成、失败和等待确认。
- 接入 Codex、ZCode 和 Git 事件。
- 在「陪伴」设置中添加图片角色、导入角色包、创建动画草稿。
- 调整动作、声音、通知和开机自启。

## 平台与安装

- **macOS（Apple Silicon）**：已验证。发布版本提供 DMG 安装包，打开后将 DeskBuddy 拖入「应用程序」目录。
- **Windows（10/11 x64）**：透明置顶窗口、拖动（含多显示器）、悬停反应、视线跟随、提示音、文件管理器定位、NSIS 安装包与应用内更新均可用。配置与会话数据位于 `%USERPROFILE%\.config\deskbuddy\`；ZCode 会话真实名称依赖 PATH 中的 `sqlite3`（未安装时回退任务摘要）。

详细操作与工具接入见 [使用手册](docs/使用手册.md)。

## 从源码运行

需要 Node.js、npm、Rust（Windows 需 MSVC 工具链，macOS 需 Xcode Command Line Tools）。

```sh
npm ci
npm run tauri -- dev
```

构建应用与安装包：

```sh
npm run tauri -- build
```

产物位于 `src-tauri/target/release/bundle/`。

## 开发与贡献

技术栈：Tauri 2、Rust、TypeScript、PixiJS、Vite。

- [架构说明](docs/architecture.md)
- [行为与事件配置](docs/configuration.md)
- [技术决策](docs/decisions.md)
- [角色包格式](assets/pets/README.md)
- [更新日志](CHANGELOG.md)

提交问题时请说明系统版本、复现步骤及预期行为。共享日志前，请移除会话内容、访问令牌和个人文件路径。

修改代码后，运行相关检查：

```sh
npm run build
node --test tests/*.test.mjs
python3 -m unittest discover -s tests -p 'test_*.py'
cargo test --manifest-path src-tauri/Cargo.toml
```

## 本地数据

配置、外部角色及运行日志位于 `~/.config/deskbuddy/`。事件服务器只监听 `127.0.0.1:17321`。应用会读取支持的编码工具本地会话记录，用于显示工作状态；日志也可能包含会话文本。

## 许可

项目采用 AGPL-3.0，完整条款见 [LICENSE](LICENSE)。导入的第三方角色素材应遵守其各自的授权条件。

应用内更新与维护者发布流程见 [发布说明](docs/releases/README.md)。

欢迎参与贡献，开发与提交检查见 [贡献指南](CONTRIBUTING.md)。

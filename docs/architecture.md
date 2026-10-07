# 架构说明

DeskBuddy 使用 Tauri 2 将 Rust 后端与 TypeScript/PixiJS 前端组合为本地桌面应用。

## 模块

| 模块 | 职责 |
|---|---|
| `src/main.ts` | 角色加载、帧动画、静态图动效、拖动与通知展示 |
| `src/bubble-view.ts`、`src/bubble.ts` | 通知布局、正文截断 |
| `src/settings.ts` | 陪伴、行为、通知、启动与显示设置 |
| `src-tauri/src/lib.rs` | 窗口、托盘、IPC 命令注册 |
| `config.rs`、`settings_cmd.rs` | 配置读写、角色管理 |
| `server.rs`、`cli.rs` | 本地 HTTP 事件接入和命令行控制 |
| `watcher.rs`、`session.rs`、`tail.rs` | 会话记录跟踪、解析和增量读取 |
| `mood.rs`、`sound.rs` | 心情分类和提示音 |
| `drag.rs`、`window_patch.rs`、`notify.rs` | 原生窗口交互与通知状态 |
| `adapters/` | 编码工具与 Git 事件适配器 |

## 事件路径

编码工具事件或本地会话记录 → Rust 状态处理 → Tauri 事件 → 前端通知与角色动作。

HTTP 服务器监听 `127.0.0.1:17321`。控制接口使用首次启动生成的本地令牌，存于 `~/.config/deskbuddy/token`。调用者可通过 CLI 接入，无需手动管理 HTTP 请求。

会话通知传递来源、会话标识、会话名称与正文。前台应用检测、通知清除和窗口层级调整包含 macOS 专用实现。

## 配置与资源

- 行为与事件映射见 [配置说明](configuration.md)。
- 外部角色保存在 `~/.config/deskbuddy/pets/`，格式见 [角色包说明](../assets/pets/README.md)。
- Vite 会把 `assets/` 内的文件复制到构建输出。放在该目录的素材即使被 Git 忽略，仍可能进入安装包。
- `src-tauri/icons/` 存放应用图标；源文件与生成方式见 [品牌资源说明](../assets/brand/README.md)。

开发环境与构建命令见 [README](../README.md)。

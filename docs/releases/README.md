# 发布与应用内更新

桌伴通过公开 GitHub Releases 提供更新。设置 → 关于会检查 `latest.json`，验证签名后下载并安装 macOS 更新包；安装完成后点击“重启使用新版”。网络错误不会被当成“已是最新版”。

公开仓库提供 [自动发布工作流模板](release-workflow.yml.example)。当前版本由维护者在本地签名并发布；启用自动发布前，将模板复制为 `.github/workflows/release.yml`，并完成下面的密钥配置。上传工作流的 GitHub 凭据需要 Workflows 写入权限。

## 维护者首次配置

1. 使用 Tauri CLI 的 `signer generate` 生成更新签名密钥，私钥保存在源码目录之外。
2. 将公钥写入 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。仓库目前内置桌伴官方发布公钥；自行分发的分支应使用自己的密钥和更新地址。
3. 在公开仓库的 Settings → Secrets and variables → Actions 添加 `TAURI_SIGNING_PRIVATE_KEY` Secret，值为私钥文件内容。加密私钥还需配置 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。不要把私钥写入仓库、文档或工作流。
4. 更新地址必须指向公开仓库的 `releases/latest/download/latest.json`，无需向客户端提供 GitHub token。

更新签名与 Apple 代码签名、公证是两套独立机制。更新签名是必要的；Apple 签名和公证可通过工作流中的 `APPLE_*` Secrets 配置。应妥善备份更新私钥，丢失后旧客户端将无法信任新密钥签发的更新。

## 发布新版本

1. 同步修改 `package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 和 `src-tauri/tauri.conf.json` 中的应用版本。
2. 创建 `docs/releases/v<版本>.md` 更新说明，完成测试并提交。
3. 自动发布工作流已启用并配置密钥后，推送对应 `v<版本>` 标签。Release 工作流会测试、构建和签名，再上传 DMG、`.app.tar.gz`、`.sig` 与 `latest.json`；所有文件上传完成后才公开 Release。
4. 检查 Release 工作流结果，并用上一版本实际验证检查、下载、安装和重启。

已存在的 Release 不会被自动覆盖。工作流失败后若留下草稿，先检查并处理草稿，再重试发布。

## 本地签名构建

设置 `TAURI_SIGNING_PRIVATE_KEY` 为私钥文件路径，并设置对应密码环境变量，运行 `npm run tauri -- build`。macOS 更新包位于 `src-tauri/target/release/bundle/macos/DeskBuddy.app.tar.gz`，其签名位于同名 `.sig` 文件。

使用 `scripts/update_manifest.py` 根据版本标签、发布仓库、更新包、签名和更新说明生成 `latest.json`；脚本会拒绝与应用版本或更新地址不一致的发布配置。

0.5.x 未包含更新器，需要手动安装 0.6.0 一次。只有更新功能所在版本已经安装、且公开 Release 和签名更新文件可访问时，应用内更新才能使用。

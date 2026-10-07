# 参与贡献

欢迎提交问题和改进。报告问题时请说明桌伴版本、macOS 版本、操作步骤、预期结果和实际结果；日志与截图请先移除私人信息。

## 本地开发

目前主要支持 macOS Apple Silicon。安装 Node.js、Rust 和 Xcode Command Line Tools 后运行：

```sh
npm ci
npm run tauri -- dev
```

## 提交前检查

```sh
node --test tests/*.test.mjs
python3 -m unittest discover -s tests -p 'test_*.py'
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm run build
```

修改角色加载、导入或更新流程时，请补充覆盖用户行为的回归测试。请勿提交私人角色、原始照片、制作记录、令牌、签名私钥或本机日志。

提交 Pull Request 时说明解决的问题、用户可观察到的变化和验证方式。发布流程见 [发布说明](docs/releases/README.md)。

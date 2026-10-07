# DeskBuddy 品牌图标

主图标为 `deskbuddy-logo.png`：奶油白立体小伙伴趴在暖橙色桌沿上。
该图由 OpenAI 图像生成工具生成，经作者在 2026-10-04 选定。

保留主图作为重新导出图标的源文件，不直接修改各尺寸的派生文件。

在项目根目录重新生成 macOS、Windows 和其他平台图标：

```sh
npm run tauri -- icon assets/brand/deskbuddy-logo.png
npm run tauri -- icon assets/brand/deskbuddy-logo.png --png 64
```

生成文件位于 `src-tauri/icons/`。应用打包图标与当前托盘图标共用这套资源。

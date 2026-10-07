# 行为与事件配置

首次启动时，应用在 `~/.config/deskbuddy/` 创建 `behavior.json` 与 `events.json`。设置页会写入这些配置，应用约每两秒检查文件变化。

## behavior.json

以下示例与当前配置字段一致：

```json
{
  "carousel": {
    "min_interval_sec": 20,
    "trigger_probability": 0.3,
    "weights": {
      "happy": { "jumping": 3, "waving": 2 },
      "normal": { "waving": 3, "jumping": 1, "crawl": 2 },
      "sad": { "waving": 2, "jumping": 1 }
    }
  },
  "hover": { "enabled": true, "action": "jumping", "guard_ms": 600 },
  "bubble": { "duration_sec": 6 },
  "mood": { "regression_per_min": 2, "neutral": 50 },
  "sound": { "enabled": true }
}
```

`weights` 设置不同心情下的动作权重。`joy` 不低于 70 时为 `happy`，不高于 40 时为 `sad`，其余为 `normal`。心情值会随时间回归中性。

## events.json

```json
{
  "task.completed": {
    "enabled": true,
    "mood": { "joy": 15, "energy": 10 },
    "action": "jumping",
    "bubble": "✓ {title}"
  },
  "task.failed": {
    "enabled": true,
    "mood": { "joy": -20 },
    "action": "failed",
    "bubble": "✗ {title}"
  },
  "agent.working": {
    "enabled": true,
    "mood": { "energy": 5 },
    "action": "running"
  },
  "agent.waiting": {
    "enabled": true,
    "mood": {},
    "action": "waiting",
    "bubble": "在等你确认：{title}"
  }
}
```

`enabled` 控制反应开关，`mood` 为心情增量，`action` 指定动作。气泡模板支持 `{title}` 插值。角色可用动作取决于其素材包；静态图角色使用程序化动效。

CLI 和工具接入见 [使用手册](使用手册.md)。

# 角色包格式

把角色包放成子目录即可被识别（托盘 → 切换角色切换，或 `deskbuddy pet <id>`）：

```
~/.config/deskbuddy/pets/
└── my-pet/
    ├── pet.json          # 必需
    ├── frames/<动作>/    # 方式A：独立帧目录（00.png 起编号）
    └── spritesheet.png   # 方式B：Petdex 雪碧图（无 x-actions 时按 8列×9行 切）
```

方式A 的 pet.json 示例（推荐，动作参数可逐个调）：

```json
{
  "id": "my-pet", "displayName": "示例角色",
  "frameWidth": 192, "frameHeight": 208,
  "gridColumns": 8, "gridRows": 9,
  "x-actions": {
    "idle":     { "dir": "frames/idle",     "fps": 5, "loop": true },
    "waving":   { "dir": "frames/waving",   "fps": 7, "loop": false },
    "jumping":  { "dir": "frames/jumping",  "fps": 10, "loop": false }
  }
}
```

方式B（纯 Petdex 包）：pet.json 带.spritesheetPath/.gridColumns/.frameWidth/.frameHeight 即可，
行序默认 [idle, running-right, running-left, waving, jumping, failed, waiting, running, review]，
可用 "x-rowOrder": [...] 覆盖。

动作名保留字（有内置语义）：idle / waiting / running / running-left / running-right /
review / failed / waving / jumping。

## 预置资源

`baby/` 是随项目公开发布的默认动画角色，包含 `pet.json`、雪碧图和动作帧。不包含原始照片或制作过程记录。

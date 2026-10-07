#!/bin/bash
# ZCode hooks 桥 v4（与文件 watcher 分工）
# watcher（rollout/model-io）负责 处理中/完成（有全文可提取）；
# hooks 只负责文件里没有的事件：waiting(等你输入) / failed(被阻塞)
DB="${DESKBUDDY_BIN:-/Applications/DeskBuddy.app/Contents/MacOS/deskbuddy}"
LOG="${DESKBUDDY_ADAPTER_LOG:-$HOME/.config/deskbuddy/adapter.log}"
mkdir -p "$(dirname "$LOG")"

# 标准输入有会话上下文时一并传递；无上下文调用继续可用。
CONTEXT=""
if [ ! -t 0 ]; then CONTEXT=$(cat); fi
/usr/bin/python3 - "$DB" "${1:-}" "$CONTEXT" >/dev/null 2>&1 <<'PYTHON'
import json, subprocess, sys
try:
    payload = json.loads(sys.argv[3] or "{}")
except ValueError:
    payload = {}
if not isinstance(payload, dict):
    payload = {}
event, detail = {"waiting": ("agent.waiting", "权限确认"),
                 "failed": ("task.failed", "被阻塞")}.get(sys.argv[2], ("", ""))
if event:
    thread_id = payload.get("sessionId") or payload.get("session_id") or ""
    thread_name = payload.get("threadName") or payload.get("thread_name") or ""
    subprocess.run([sys.argv[1], "emit", event, "--source", "zcode",
                    "--thread-id", str(thread_id), "--thread-name", str(thread_name),
                    "--detail", detail], check=False)
PYTHON
echo "$(date '+%H:%M:%S') zcode-hook ${1:-} 完成" >>"$LOG"
exit 0

#!/bin/bash
# Codex notify 包装器 v2（桌伴接入·状态级通知）
# 可选转发给已有通知程序；任务完成时向桌伴发送会话状态
FORWARD_BIN="${DESKBUDDY_FORWARD_BIN:-}"
DB="${DESKBUDDY_BIN:-/Applications/DeskBuddy.app/Contents/MacOS/deskbuddy}"
PY=/usr/bin/python3
LOG="${DESKBUDDY_ADAPTER_LOG:-$HOME/.config/deskbuddy/adapter.log}"
mkdir -p "$(dirname "$LOG")"
echo "$(date '+%H:%M:%S') codex-notify 被调用" >>"$LOG"

# 1) 可选转发（通过 DESKBUDDY_FORWARD_BIN 配置）
if [ -x "$FORWARD_BIN" ]; then
  "$FORWARD_BIN" "$@" >/dev/null 2>&1 &
fi

# 2) 完成只发送一次：会话身份与真实回复一起传递。
JSON="${@: -1}"
"$PY" - "$DB" "$JSON" >>"$LOG" 2>&1 <<'PYTHON'
import json, subprocess, sys
try:
    payload = json.loads(sys.argv[2])
except (ValueError, IndexError):
    sys.exit(0)
if isinstance(payload, dict) and payload.get("type") == "agent-turn-complete":
    thread_id = payload.get("thread-id") or payload.get("threadId") or ""
    thread_name = payload.get("thread-name") or payload.get("threadName") or ""
    reply = payload.get("last-assistant-message") or "任务完成，等待查看"
    subprocess.run([sys.argv[1], "emit", "task.completed", "--source", "codex",
                    "--thread-id", str(thread_id), "--thread-name", str(thread_name),
                    "--detail", str(reply)], check=False)
PYTHON
exit 0

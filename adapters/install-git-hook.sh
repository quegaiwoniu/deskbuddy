#!/bin/bash
# 桌伴 git hook 安装器（幂等·追加式·不动已有内容）
# 用法: bash install-git-hook.sh <仓库路径>   （默认当前目录）
# 效果: 每次提交 → 宝宝挥手 + 气泡"✓ 提交：<摘要>"
REPO="${1:-.}"
HOOK="$REPO/.git/hooks/post-commit"
DB="/Applications/DeskBuddy.app/Contents/MacOS/deskbuddy"
MARKER="# deskbuddy-hook"

[ -d "$REPO/.git" ] || { echo "不是 git 仓库: $REPO"; exit 1; }
touch "$HOOK" && chmod +x "$HOOK"
grep -q "$MARKER" "$HOOK" && { echo "已安装过，跳过: $HOOK"; exit 0; }
cat >> "$HOOK" <<EOS
$MARKER
if [ -x "$DB" ]; then
  SUBJ=\$(git log -1 --pretty=%s | head -c 24)
  "\$DB" emit task.completed --action waving --source git --title "提交：\$SUBJ" >/dev/null 2>&1
fi
EOS
echo "已安装: $HOOK"

"""Verify adapters without touching the running pet or Computer Use."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
ROOT = Path(__file__).resolve().parents[1]
class AdaptersTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.calls = self.directory / "calls.jsonl"
        self.fake = self.directory / "deskbuddy"
        self.fake.write_text('#!/usr/bin/python3\nimport json,os,sys\nwith open(os.environ["DB_CALL_LOG"],"a") as f:f.write(json.dumps(sys.argv[1:])+"\\n")\n')
        self.fake.chmod(0o755)
        self.env = dict(os.environ, DESKBUDDY_BIN=str(self.fake), DESKBUDDY_FORWARD_BIN="/nonexistent",
                        DESKBUDDY_ADAPTER_LOG=str(self.directory / "adapter.log"), DB_CALL_LOG=str(self.calls))
    def run_hook(self, name, args, data=""):
        subprocess.run(["bash", str(ROOT / "adapters" / name), *args],
                       input=data, text=True, env=self.env, check=True)
        return [json.loads(line) for line in self.calls.read_text().splitlines()] if self.calls.exists() else []
    def test_codex_complete_sends_exactly_one_unmodified_reply(self):
        reply = '中文回复 👨‍👩‍👧‍👦\n第二行，字面量 $(echo unsafe) ' + chr(96) + 'command' + chr(96)
        calls = self.run_hook("codex-notify.sh", [json.dumps({
            "type": "agent-turn-complete", "thread-id": "thread-a", "last-assistant-message": reply})])
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][:4], ["emit", "task.completed", "--source", "codex"])
        self.assertEqual(calls[0][calls[0].index("--thread-id") + 1], "thread-a")
        self.assertEqual(calls[0][calls[0].index("--detail") + 1], reply)
    def test_codex_non_completion_and_invalid_payload_are_ignored(self):
        self.assertEqual(self.run_hook("codex-notify.sh", ['{"type":"other"}']), [])
        self.assertEqual(self.run_hook("codex-notify.sh", ["not-json"]), [])
    def test_zcode_waiting_preserves_session_context(self):
        calls = self.run_hook("zcode-hook.sh", ["waiting"], json.dumps({"sessionId": "sess-a", "threadName": "我的会话"}))
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][1], "agent.waiting")
        self.assertEqual(calls[0][calls[0].index("--thread-name") + 1], "我的会话")
    def test_zcode_failed_without_context_sends_only_one_event(self):
        calls = self.run_hook("zcode-hook.sh", ["failed"])
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][1], "task.failed")
if __name__ == "__main__":
    unittest.main()

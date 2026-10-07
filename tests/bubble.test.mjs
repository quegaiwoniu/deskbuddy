import { test } from "node:test";
import assert from "node:assert/strict";
import ts from "typescript";
import { readFileSync } from "node:fs";
const source = ts.transpileModule(readFileSync(new URL("../src/bubble.ts", import.meta.url), "utf8"), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
const { fitBubbleText } = await import("data:text/javascript;base64," + Buffer.from(source).toString("base64"));
const segments = new Intl.Segmenter("zh", { granularity: "grapheme" });
const measure = s => Array.from(segments.segment(s)).length * 10;
test("long title and body are clipped independently within their width", () => {
  const title = fitBubbleText("优化一个非常长的会话名称", 60, measure);
  const body = fitBubbleText("检查结果没有错误，正在验证布局", 100, measure);
  assert.ok(measure(title) <= 60);
  assert.ok(measure(body) <= 100);
  assert.ok(title.endsWith("…"));
  assert.ok(body.endsWith("…"));
});
test("clipping preserves joined emoji and combining marks", () => {
  assert.equal(fitBubbleText("👨‍👩‍👧‍👦宝宝e\u0301测试", 30, measure), "👨‍👩‍👧‍👦宝…");
  assert.equal(fitBubbleText("e\u0301测试结果", 20, measure), "e\u0301…");
});
test("normalizes whitespace and fits short text without ellipsis", () => {
  assert.equal(fitBubbleText(" 已完成\n 检查 ", 200, measure), "已完成 检查");
  assert.equal(fitBubbleText("内容", 0, measure), "");
});

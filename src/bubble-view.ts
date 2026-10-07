import { Container, Graphics, Text } from "pixi.js";
import { fitBubbleText } from "./bubble";
export type StatusMsg = { icon: string; title: string; detail: string; tone: string; threadId?: string };
export const BUBBLE_WIDTH = 344;
export const BUBBLE_HEIGHT = 52;
const COLORS: Record<string, number> = { good: 0x34c759, bad: 0xff453a, work: 0x6e6e73, wait: 0xff9f0a, plain: 0x1d1d1f };
export function createBubbleView() {
  const container = new Container();
  const background = new Graphics();
  const style = { fontFamily: "PingFang SC, system-ui, sans-serif", fontSize: 12, fill: 0x1d1d1f };
  const title = new Text({ text: "", style: { ...style, fontWeight: "600" } });
  const icon = new Text({ text: "", style });
  const detail = new Text({ text: "", style: { ...style, fontSize: 11, fill: 0x4a4a4f } });
  const spinner = new Graphics();
  spinner.circle(0, 0, 5.5).stroke({ width: 2.5, color: 0x8e8e93, alpha: 0.3 });
  spinner.arc(0, 0, 5.5, 0, Math.PI * 0.7).stroke({ width: 2.5, color: 0x8e8e93, cap: "round" });
  spinner.position.set(19, 16);
  container.addChild(background, icon, title, detail, spinner);
  function show(message: StatusMsg) {
    const work = message.tone === "work";
    spinner.visible = work;
    icon.text = work ? "" : message.icon;
    icon.style.fill = COLORS[message.tone] ?? COLORS.plain;
    icon.position.set(12, 8);
    const textX = work || message.icon ? 36 : 12;
    title.style.fill = COLORS.plain;
    title.text = fitBubbleText(message.title, BUBBLE_WIDTH - 12 - textX, value => { title.text = value; return title.width; });
    const body = message.detail?.trim() || (message.tone === "good" ? "任务完成，等待查看" : work ? "正在处理…" : "");
    detail.text = fitBubbleText(body, BUBBLE_WIDTH - 12 - textX, value => { detail.text = value; return detail.width; });
    title.position.set(textX, 8);
    detail.position.set(textX, 29);
    background.clear().roundRect(0, 0, BUBBLE_WIDTH, BUBBLE_HEIGHT, 10)
      .fill({ color: 0xffffff, alpha: 0.97 }).stroke({ width: 1, color: 0xd2d2d7 });
  }
  return { container, title, detail, spinner, show };
}

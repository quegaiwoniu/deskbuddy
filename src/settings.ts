import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { UpdateController } from "./updater";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

type Weights = Record<string, number>;
type Behavior = {
  carousel: { min_interval_sec: number; trigger_probability: number; weights: Record<string, Weights> };
  hover: { enabled: boolean; action: string; guard_ms: number };
  bubble: { duration_sec: number };
  mood: { regression_per_min: number; neutral: number };
  sound: { enabled: boolean };
};
type EventMapping = { mood: Record<string, number>; action?: string | null; bubble?: string | null; enabled: boolean };
type PetEntry = { id: string; name: string; current: boolean; draft: boolean };
type Page = "pets" | "behavior" | "events" | "general" | "about";
type CreateMode = "image" | "draft" | "import" | null;

const app = document.getElementById("app")!;
const nav = document.getElementById("nav")!;
const pages: [Page, string][] = [["pets", "陪伴"], ["behavior", "行为"], ["events", "通知"], ["general", "启动与显示"], ["about", "关于"]];
const eventNames: [string, string, string][] = [
  ["task.completed", "任务完成", "播放庆祝动作并显示回复"],
  ["task.failed", "任务失败", "提示需要关注"],
  ["agent.working", "正在处理", "显示会话进展"],
  ["agent.waiting", "等待处理", "提示需要你操作"],
];
let page: Page = "pets";
let appVersion = "";
const updater = new UpdateController({
  check: () => check({ timeout: 20000 }),
  restart: relaunch,
}, () => { if (page === "about") render(); });
void getVersion().then(version => { appVersion = version; if (page === "about") render(); }).catch(() => {});
let behavior!: Behavior;
let events!: Record<string, EventMapping>;
let pets: PetEntry[] = [];
let autostart = false;
let createMode: CreateMode = null;
let draftId: string | null = null;
let openMenu: string | null = null;
let deleteId: string | null = null;
let notice = "";
let noticeError = false;
let busy = false;
let behaviorSaveQueue: Promise<unknown> = Promise.resolve();
let eventsSaveQueue: Promise<unknown> = Promise.resolve();

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = text;
  return node;
}
function button(text: string, onClick: () => void, className = ""): HTMLButtonElement {
  const b = el("button", className, text);
  b.type = "button";
  b.onclick = onClick;
  return b;
}
function header(title: string, action?: HTMLElement): HTMLElement {
  const h = el("div", "page-head");
  h.append(el("h1", "", title));
  if (action) h.append(action);
  return h;
}
function panel(): HTMLElement { return el("section", "panel"); }
function sectionTitle(text: string): HTMLElement { return el("h2", "", text); }
function message(text: string, isError = false): HTMLElement {
  return el("p", isError ? "status error" : "status", text);
}
function statusText(status: HTMLElement, text: string, error = false) {
  status.textContent = text;
  status.classList.toggle("error", error);
}
async function saveSetting<T>(status: HTMLElement, control: HTMLInputElement | HTMLSelectElement, save: () => Promise<T>, rollback: () => void) {
  control.disabled = true;
  statusText(status, "保存中…");
  try {
    await save();
    statusText(status, "已保存");
    setTimeout(() => { if (status.textContent === "已保存") statusText(status, ""); }, 1800);
  } catch (e) {
    rollback();
    statusText(status, `保存失败：${e}`, true);
  } finally { control.disabled = false; }
}
function settingRow(title: string, sub: string, control: HTMLElement): { row: HTMLElement; status: HTMLElement } {
  const row = el("div", "setting-row");
  const copy = el("div", "setting-copy");
  copy.append(el("div", "setting-title", title), el("div", "sub", sub));
  const status = el("div", "status");
  copy.append(status);
  const controls = el("div", "controls");
  controls.append(control);
  row.append(copy, controls);
  return { row, status };
}
function checkboxRow(title: string, sub: string, initial: boolean, update: (value: boolean) => Promise<unknown>) {
  const input = el("input") as HTMLInputElement;
  input.type = "checkbox";
  input.checked = initial;
  input.setAttribute("aria-label", title);
  const { row, status } = settingRow(title, sub, input);
  input.onchange = () => {
    const previous = !input.checked;
    void saveSetting(status, input, () => update(input.checked), () => { input.checked = previous; });
  };
  return row;
}
function sliderRow(title: string, sub: string, value: number, min: number, max: number, step: number, format: (v: number) => string, update: (value: number) => Promise<unknown>) {
  const wrap = el("div", "controls");
  const input = el("input") as HTMLInputElement;
  input.type = "range"; input.min = String(min); input.max = String(max); input.step = String(step);
  input.value = String(Number.isFinite(value) ? value : min);
  input.setAttribute("aria-label", title);
  const valueEl = el("span", "value", format(Number(input.value)));
  wrap.append(input, valueEl);
  const { row, status } = settingRow(title, sub, wrap);
  let saved = Number(input.value);
  input.oninput = () => { valueEl.textContent = format(Number(input.value)); };
  input.onchange = () => {
    const next = Number(input.value);
    void saveSetting(status, input, async () => { await update(next); saved = next; }, () => {
      input.value = String(saved);
      valueEl.textContent = format(saved);
    });
  };
  return row;
}
function selectRow(title: string, sub: string, value: string, options: [string, string][], update: (value: string) => Promise<unknown>) {
  const sel = el("select") as HTMLSelectElement;
  for (const [val, label] of options) {
    const opt = el("option", "", label);
    opt.value = val;
    sel.append(opt);
  }
  sel.value = value;
  sel.setAttribute("aria-label", title);
  const { row, status } = settingRow(title, sub, sel);
  let saved = value;
  sel.onchange = () => {
    void saveSetting(status, sel, async () => { await update(sel.value); saved = sel.value; }, () => { sel.value = saved; });
  };
  return row;
}
function updateBehavior(mutate: (b: Behavior) => void): Promise<unknown> {
  const work = behaviorSaveQueue.then(async () => {
    const next = structuredClone(behavior);
    mutate(next);
    await invoke("save_behavior", { cfg: next });
    behavior = next;
  });
  behaviorSaveQueue = work.catch(() => {});
  return work;
}
function updateEvent(key: string, enabled: boolean): Promise<unknown> {
  const work = eventsSaveQueue.then(async () => {
    const next = structuredClone(events);
    next[key] = next[key] ?? { mood: {}, action: null, bubble: null, enabled: true };
    next[key].enabled = enabled;
    await invoke("save_events", { cfg: next });
    events = next;
  });
  eventsSaveQueue = work.catch(() => {});
  return work;
}
async function refreshPets() { pets = await invoke<PetEntry[]>("list_pets"); }
async function petAction(operation: () => Promise<unknown>, success: string, preserveOnError?: HTMLElement, onSuccess?: (result: unknown) => void) {
  if (busy) return;
  busy = true;
  app.setAttribute("aria-busy", "true");
  let succeeded = false;
  try {
    const result = await operation();
    await refreshPets();
    notice = success; noticeError = false;
    openMenu = null; deleteId = null; createMode = null; draftId = null;
    onSuccess?.(result);
    succeeded = true;
  } catch (e) {
    notice = String(e); noticeError = true;
  } finally {
    busy = false;
    app.removeAttribute("aria-busy");
    if (succeeded || !preserveOnError) render();
    else statusText(preserveOnError, notice, true);
  }
}
function renderNav() {
  nav.replaceChildren();
  for (const [id, label] of pages) {
    const b = button(label, () => { page = id; createMode = null; draftId = null; deleteId = null; notice = ""; render(); });
    if (page === id) b.setAttribute("aria-current", "page");
    nav.append(b);
  }
}
function petRow(p: PetEntry): HTMLElement {
  const row = el("div", "pet-row");
  const dot = el("span", p.current ? "dot active" : "dot");
  const copy = el("div", "pet-copy");
  const name = el("span", "pet-name", p.name);
  copy.append(name);
  if (p.id === "baby") copy.append(el("span", "badge", "内置"));
  if (p.current) copy.append(el("span", "badge", "当前"));
  if (p.draft) copy.append(el("span", "badge", "草稿"));
  const actions = el("div", "pet-actions");
  if (p.draft) actions.append(button("继续制作", () => { createMode = null; deleteId = null; draftId = p.id; render(); }));
  else if (!p.current) actions.append(button("使用", () => void petAction(() => invoke("select_pet", { id: p.id }), `已切换到「${p.name}」`)));
  if (p.id !== "baby") {
    const more = button("···", () => { openMenu = openMenu === p.id ? null : p.id; render(); }, "ghost more");
    more.setAttribute("aria-label", `${p.name}的更多操作`);
    more.setAttribute("aria-expanded", String(openMenu === p.id));
    actions.append(more);
    if (openMenu === p.id) {
      const menu = el("div", "popover");
      menu.append(
        button("在访达中显示", () => void petAction(() => invoke("show_pet_in_folder", { id: p.id }), "已在访达中显示")),
        button("删除角色…", () => { openMenu = null; deleteId = p.id; render(); }, "danger"),
      );
      row.append(menu);
    }
  }
  row.append(dot, copy, actions);
  return row;
}
function deleteConfirm(p: PetEntry): HTMLElement {
  const c = el("div", "confirm");
  c.append(el("strong", "", `删除「${p.name}」？`));
  c.append(el("div", "sub", p.current ? "会先切换到内置宝宝，然后删除该角色的本地文件。此操作无法撤销。" : "该角色的本地文件会被删除。此操作无法撤销。"));
  const actions = el("div", "inline-actions");
  const remove = button("确认删除", () => void petAction(() => invoke("delete_pet", { id: p.id }), `已删除「${p.name}」`), "danger");
  remove.disabled = busy;
  actions.append(button("取消", () => { deleteId = null; render(); }), remove);
  c.append(actions);
  return c;
}
function showDraft(p: PetEntry) {
  app.replaceChildren(header(`继续制作 · ${p.name}`, button("返回陪伴", () => { draftId = null; notice = ""; render(); })));
  app.append(el("p", "step", "第 2 步 / 共 3 步 · 生成素材并放入 raw 文件夹"));
  const box = panel();
  box.append(el("p", "", "打开提示词和素材文件夹，按 PROMPTS.md 生成动画，把文件放入 raw 文件夹。完成后返回此处组装。"));
  const actions = el("div", "form-actions");
  actions.append(
    button("打开素材文件夹", () => {
      void invoke("open_pet_raw_folder", { id: p.id })
        .then(() => { notice = "已在访达中显示"; noticeError = false; render(); })
        .catch(e => { notice = String(e); noticeError = true; render(); });
    }),
    button("检查并组装", () => void petAction(() => invoke("assemble_pet", { id: p.id }), `「${p.name}」已组装`), "primary"),
  );
  box.append(actions);
  app.append(box);
  if (notice) app.append(message(notice, noticeError));
}
function renderPets() {
  if (draftId) {
    const draft = pets.find(p => p.id === draftId && p.draft);
    if (draft) { showDraft(draft); return; }
    draftId = null;
  }
  app.append(header("我的陪伴", button("＋ 添加角色", () => { createMode = "image"; notice = ""; render(); }, "primary")));
  if (createMode) { renderCreate(); return; }
  if (notice) app.append(message(notice, noticeError));
  app.append(sectionTitle("可使用"));
  const ready = panel();
  for (const p of pets.filter(p => !p.draft)) {
    ready.append(petRow(p));
    if (deleteId === p.id) ready.append(deleteConfirm(p));
  }
  if (pets.every(p => p.id === "baby")) ready.append(el("p", "empty", "还没有自定义角色。点击右上角添加。"));
  app.append(ready);
  const drafts = pets.filter(p => p.draft);
  if (drafts.length) {
    app.append(sectionTitle("制作中"));
    const draftPanel = panel();
    for (const p of drafts) {
      draftPanel.append(petRow(p));
      if (deleteId === p.id) draftPanel.append(deleteConfirm(p));
    }
    app.append(draftPanel);
  }
  const details = el("details", "advanced");
  details.append(el("summary", "", "高级操作"));
  details.append(button("打开角色文件夹", () => void petAction(() => invoke("open_pets_folder"), "已打开角色文件夹")));
  app.append(details);
}
function renderCreate() {
  app.replaceChildren(header("添加角色", button("取消", () => { createMode = null; notice = ""; render(); })));
  app.append(el("p", "step", "第 1 步 / 共 3 步 · 选择创建方式"));
  const options = panel();
  const modes: [CreateMode, string, string][] = [
    ["image", "使用一张图片", "选择本地 PNG、WebP 或 JPEG，创建静态角色。"],
    ["draft", "生成动画草稿", "创建提示词和素材目录，稍后组装动画。"],
    ["import", "导入角色包", "导入已有角色的完整文件夹，自动识别角色信息和素材。"],
  ];
  for (const [mode, title, sub] of modes) {
    const b = button("", () => { createMode = mode; notice = ""; render(); }, "choice");
    b.append(el("strong", "", `${createMode === mode ? "● " : ""}${title}`), el("span", "sub", sub));
    options.append(b);
  }
  app.append(options);
  app.append(el("p", "step", createMode === "import" ? "第 2 步 / 共 3 步 · 选择角色包" : "第 2 步 / 共 3 步 · 填写信息"));
  const form = panel();
  const field = el("label", "field", "角色名称");
  const name = el("input") as HTMLInputElement;
  name.type = "text"; name.placeholder = "给角色起个名字"; name.maxLength = 60;
  field.append(name);
  if (createMode !== "import") form.append(field);
  let desc: HTMLTextAreaElement | null = null;
  if (createMode === "draft") {
    const descLabel = el("label", "field", "外貌与风格");
    desc = el("textarea") as HTMLTextAreaElement;
    desc.placeholder = "例如：橘色小猫，圆脸，戴蓝围巾，软萌贴纸风";
    descLabel.append(desc);
    form.append(descLabel);
  }
  if (createMode === "import") form.append(el("p", "sub", "选择包含 pet.json 的角色根文件夹，例如“小宝”，不要选择 frames 子目录。桌伴只复制角色运行所需的图片和动画帧，跳过制作记录与预览文件，原文件保持不变。"));
  const actions = el("div", "form-actions");
  const create = button(createMode === "image" ? "选择图片并创建" : createMode === "draft" ? "创建草稿" : "选择文件夹并导入", async () => {
    const value = name.value.trim();
    if (createMode !== "import" && !value) { statusText(feedback, "请填写角色名称", true); name.focus(); return; }
    if (createMode === "draft" && !desc?.value.trim()) { statusText(feedback, "请描述角色的外貌与风格", true); desc?.focus(); return; }
    if (busy) return;
    if (createMode === "draft") {
      await petAction(() => invoke("create_draft_pet", { name: value, description: desc!.value.trim() }), "草稿已创建，请生成素材并放入 raw 文件夹", feedback, id => { draftId = String(id); });
    } else if (createMode === "image") {
      const path = await open({ filters: [{ name: "图片", extensions: ["png", "webp", "jpg", "jpeg"] }] });
      if (typeof path !== "string") return;
      await petAction(async () => {
        const id = await invoke<string>("create_static_pet", { name: value, imagePath: path });
        await invoke("select_pet", { id });
      }, `「${value}」已创建并启用`, feedback);
    } else {
      const path = await open({ directory: true });
      if (typeof path !== "string") return;
      await petAction(() => invoke("import_pet_folder", { source: path }), "角色包已导入", feedback);
    }
  }, "primary");
  actions.append(create);
  const feedback = el("div", "status");
  form.append(actions, feedback);
  app.append(form);
  if (notice) app.append(message(notice, noticeError));
}
function renderBehavior() {
  app.append(header("行为"));
  app.append(sectionTitle("自主动作"));
  const c = panel();
  c.append(
    sliderRow("轮播间隔", "两次自主动作之间的最短时间", behavior.carousel.min_interval_sec, 5, 120, 1, v => `${v}秒`, v => updateBehavior(b => { b.carousel.min_interval_sec = v; })),
    sliderRow("轮播概率", "到达间隔后触发动作的概率", Math.round(behavior.carousel.trigger_probability * 100), 5, 100, 5, v => `${v}%`, v => updateBehavior(b => { b.carousel.trigger_probability = v / 100; })),
    checkboxRow("悬停反应", "鼠标靠近时播放动作", behavior.hover.enabled, v => updateBehavior(b => { b.hover.enabled = v; })),
    selectRow("悬停动作", "工作时不会打断任务动画", behavior.hover.action, [["jumping","跳跃"],["waving","挥手"],["review","看代码"],["waiting","坐等"]], v => updateBehavior(b => { b.hover.action = v; })),
  );
  app.append(c);
}
function renderEvents() {
  app.append(header("通知"));
  app.append(sectionTitle("事件反应"));
  const c = panel();
  for (const [key, title, sub] of eventNames) c.append(checkboxRow(title, sub, events[key]?.enabled ?? true, v => updateEvent(key, v)));
  app.append(c, sectionTitle("呈现方式"));
  const d = panel();
  d.append(
    sliderRow("气泡时长", "非持续事件显示多久", behavior.bubble.duration_sec, 2, 15, 1, v => `${v}秒`, v => updateBehavior(b => { b.bubble.duration_sec = v; })),
    checkboxRow("声音提示", "完成或失败时播放轻提示音", behavior.sound.enabled, v => updateBehavior(b => { b.sound.enabled = v; })),
  );
  app.append(d);
}
function renderGeneral() {
  app.append(header("启动与显示"), sectionTitle("启动"));
  const c = panel();
  c.append(checkboxRow("开机自启", "登录 macOS 后自动显示桌伴", autostart, async v => {
    await invoke("set_autostart", { enabled: v }); autostart = v;
  }));
  app.append(c, sectionTitle("高级"));
  const d = panel();
  const levels = el("div", "controls");
  const { row, status } = settingRow("窗口层级", "桌宠被其他窗口遮住时调整", levels);
  for (const [label, level] of [["标准", 25], ["高", 101], ["最高", 1000]] as const) {
    const b = button(label, () => {
      b.disabled = true;
      statusText(status, "应用中…");
      void invoke("set_window_level", { level })
        .then(() => statusText(status, `已设为${label}`))
        .catch(e => statusText(status, String(e), true))
        .finally(() => { b.disabled = false; });
    });
    levels.append(b);
  }
  d.append(row);
  app.append(d);
}
function renderAbout() {
  app.append(header("关于"));
  const c = panel();
  c.append(el("p", "", `桌伴 DeskBuddy${appVersion ? ` · v${appVersion}` : ""}`));
  c.append(el("p", "muted", "感知编码事件的桌面陪伴。配置与角色素材保存在本机。"));
  app.append(c);
  app.append(sectionTitle("软件更新"));
  const updatePanel = panel();
  const state = updater.state;
  const labels = {
    idle: "检查是否有可用的新版本。", checking: "正在检查更新…", current: "已是最新版本。",
    available: `发现新版本 v${state.version}`, downloading: "正在下载并安装…",
    ready: "新版已安装，重启后生效。", restarting: "正在重启…", error: "更新未完成，请重试。",
  };
  updatePanel.append(el("p", "setting-title", labels[state.phase]));
  if (state.notes) updatePanel.append(el("p", "release-notes", state.notes));
  if (state.phase === "downloading") {
    const progress = el("progress");
    progress.setAttribute("aria-label", "更新下载进度");
    if (state.total) { progress.max = state.total; progress.value = Math.min(state.downloaded, state.total); }
    updatePanel.append(progress);
    const mb = (bytes: number) => (bytes / 1024 / 1024).toFixed(1);
    updatePanel.append(el("p", "muted", state.total
      ? `${Math.min(100, Math.floor(state.downloaded / state.total * 100))}% · ${mb(state.downloaded)} / ${mb(state.total)} MB`
      : `已下载 ${mb(state.downloaded)} MB`));
  }
  if (state.error) updatePanel.append(el("p", "status error update-error", state.error));
  const controls = el("div", "form-actions");
  if (state.phase === "available" || (state.phase === "error" && state.operation === "install")) {
    controls.append(button(state.phase === "error" ? "重试下载" : "下载并安装", () => void updater.install(), "primary"));
    controls.append(button("重新检查", () => void updater.check()));
  } else if (state.phase === "ready" || (state.phase === "error" && state.operation === "restart")) {
    controls.append(button("重启使用新版", () => void updater.restart(), "primary"));
  } else {
    const checking = button(state.phase === "checking" ? "检查中…" : "检查更新", () => void updater.check());
    checking.disabled = ["checking", "downloading", "restarting"].includes(state.phase);
    controls.append(checking);
  }
  updatePanel.append(controls);
  app.append(updatePanel);
}
function render() {
  renderNav();
  app.replaceChildren();
  if (page === "pets") renderPets();
  else if (page === "behavior") renderBehavior();
  else if (page === "events") renderEvents();
  else if (page === "general") renderGeneral();
  else renderAbout();
  if (busy) app.setAttribute("aria-busy", "true");
  else app.removeAttribute("aria-busy");
}
async function main() {
  app.replaceChildren(header("加载设置…"));
  [behavior, events, pets, autostart] = await Promise.all([
    invoke<Behavior>("get_behavior"),
    invoke<Record<string, EventMapping>>("get_events"),
    invoke<PetEntry[]>("list_pets"),
    invoke<boolean>("get_autostart"),
  ]);
  await listen("pets-changed", () => { if (!busy) void refreshPets().then(render); });
  render();
}
document.addEventListener("keydown", e => {
  if (e.key !== "Escape") return;
  if (openMenu || deleteId || createMode || draftId) {
    openMenu = null; deleteId = null; createMode = null; draftId = null; notice = "";
    render();
  }
});
document.addEventListener("click", e => {
  const target = e.target;
  if (openMenu && target instanceof Element && !target.closest(".popover") && !target.closest(".more")) {
    openMenu = null;
    render();
  }
});
main().catch(e => { app.replaceChildren(header("设置加载失败"), message(String(e), true), button("重试", () => void main())); });

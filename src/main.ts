import { createBubbleView, BUBBLE_WIDTH, type StatusMsg } from "./bubble-view";
import { Application, AnimatedSprite, Graphics, Rectangle, Texture } from "pixi.js";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// 固定尺寸：112pt 显示高（与 Codex petSize 默认值一致，实测自 ChatGPT.app 配置定义）
const WIN_W = 352;
const WIN_H = 216;
const DISPLAY_H = 112;

// ---- 行为配置（默认值；启动后由 behavior.json 热重载覆盖）----
type Weights = Record<string, number>;
// 字段名与 behavior.json 一致（snake_case）
type Behavior = {
  carousel: { min_interval_sec: number; trigger_probability: number; weights: Record<string, Weights> };
  hover: { enabled: boolean; action: string; guard_ms: number };
  bubble: { duration_sec: number };
};
let behavior: Behavior = {
  carousel: {
    min_interval_sec: 20,
    trigger_probability: 0.3,
    weights: {
      happy: { jumping: 3, waving: 2 },
      normal: { waving: 3, jumping: 1, crawl: 2 },
      sad: { waving: 2, jumping: 1 },
    },
  },
  hover: { enabled: true, action: "jumping", guard_ms: 600 },
  bubble: { duration_sec: 6 },
};
let moodCategory = "normal";

type ActionDef = { dir: string; fps: number; loop: boolean };
type PetJson = {
  displayName: string;
  frameWidth?: number;
  frameHeight?: number;
  staticImage?: string;
  "x-actions"?: Record<string, ActionDef>;
  spritesheetPath?: string;
  gridColumns?: number;
  "x-rowOrder"?: string[];
};

type Mode = "idle" | "insert" | "drag" | "manual";
let mode: Mode = "idle";
let lastInsert = Date.now();
// 视线跟随：近距待机底座（空串=普通 idle）；resumeBase 定义在 main() 内（需访问 play/actions）
let lastLook = "";
// 持续活状态（Codex 派生态：条件在姿势在）
let liveState = "idle";
// 静态图宠物：合成动效模式
let staticMode = false;
let staticFit = 1;

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function weightedPick(weights: Record<string, number>): string {
  const entries = Object.entries(weights);
  const total = entries.reduce((s, [, w]) => s + w, 0);
  let roll = Math.random() * total;
  for (const [key, w] of entries) {
    roll -= w;
    if (roll <= 0) return key;
  }
  return entries[entries.length - 1][0];
}

async function main() {
  let pet!: PetJson;
  let actions!: Record<string, ActionDef>;
  let frames!: Record<string, Texture[]>;

  // 宠物基址 → 文件加载。全部走 IPC data URL：
  // asset:// 外部图片会使 WebGL 画布污染（texImage2D 抛 SecurityError → 每帧渲染失败）
  const fileUrl = async (base: string, rel: string): Promise<string> => {
    const relPath = base.startsWith("builtin:")
      ? base.slice("builtin:".length) + "/" + rel
      : "pets/" + base.slice("ext:".length).split("/").pop() + "/" + rel;
    return await invoke<string>("read_pet_file", { rel: relPath.replace(/^\/+/, "") });
  };

  // Petdex 雪碧图行序（无 x-actions 时按此切 9 行）
  const DEFAULT_ROW_ORDER = ["idle","running-right","running-left","waving","jumping","failed","waiting","running","review"];

  // 带重试的图片加载（冷启动偶发失败是前端整体死亡的历史根因）
  async function loadImage(url: string, tries = 3): Promise<HTMLImageElement | null> {
    if (!url) return null; // IPC 对缺失文件返回空串 = 帧序列结束
    for (let t = 0; t < tries; t++) {
      const img = new Image();
      img.src = t === 0 ? url : `${url}#r${t}`;
      try {
        await img.decode();
        return img;
      } catch {
        await sleep(150);
      }
    }
    return null;
  }

  async function loadPet(base: string) {
    const manifestUrl = await fileUrl(base, "pet.json");
    if (!manifestUrl) throw new Error(`角色包不存在: ${base}`);
    pet = await (await fetch(manifestUrl)).json();
    actions = pet["x-actions"] ?? {};
    frames = {};
    staticMode = false;
    staticFit = 1;
    if (Object.keys(actions).length > 0) {
      // 首选：独立帧目录（00.png 起连续编号）；单动作加载失败→跳过该动作而非整体崩溃
      const failed: string[] = [];
      for (const [name, def] of Object.entries(actions)) {
        const list: Texture[] = [];
        for (let i = 0; ; i++) {
          const img = await loadImage(await fileUrl(base, `${def.dir}/${String(i).padStart(2, "0")}.png`), 2);
          if (!img) break; // 重试后仍失败 → 视为帧结束
          list.push(Texture.from(img));
          if (i > 40) break; // 保险丝
        }
        if (list.length === 0) {
          failed.push(name);
          delete actions[name];
        } else {
          frames[name] = list;
        }
      }
      if (failed.length > 0) {
        (window as any).__beacon?.("pet-partial", `${base} 缺动作: ${failed.join(",")}`);
      }
      if (!frames.idle) {
        // idle 都没了：回退内置宝宝，绝不黑屏
        (window as any).__beacon?.("pet-fallback", `${base} 无 idle，回退内置`);
        if (base !== "builtin:/pets/baby") return loadPet("builtin:/pets/baby");
        throw new Error("内置角色包损坏：无 idle 帧");
      }
    } else if (pet.spritesheetPath && pet.gridColumns) {
      // 兼容：Petdex 雪碧图按网格切片（行序取 x-rowOrder 或默认）
      const img = new Image();
      img.src = await fileUrl(base, pet.spritesheetPath);
      await img.decode();
      const sheet = Texture.from(img);
      const rowOrder = pet["x-rowOrder"] ?? DEFAULT_ROW_ORDER;
      rowOrder.forEach((action, r) => {
        frames[action] = Array.from({ length: pet.gridColumns! }, (_, c) => {
          const fw = pet.frameWidth ?? 192, fh = pet.frameHeight ?? 208;
          const rect = new Rectangle(c * fw, r * fh, fw, fh);
          return new Texture({ source: sheet.source, frame: rect });
        });
      });
      const oneShot = new Set(["waving", "jumping"]);
      actions = Object.fromEntries(
        Object.keys(frames).map((a) => [a, { dir: "", fps: oneShot.has(a) ? 9 : 6, loop: !oneShot.has(a) }]),
      );
    } else if (pet.staticImage) {
      // 静态图宠物：单纹理 + 程序化动效（合成动作引擎）
      const img = await loadImage(await fileUrl(base, pet.staticImage), 3);
      if (!img) throw new Error(`静态图加载失败: ${pet.staticImage}`);
      const tex = Texture.from(img);
      const all = ["idle","waiting","running","running-left","running-right","review","failed","waving","jumping","look-up","look-down","look-left","look-right"];
      for (const a of all) {
        frames[a] = [tex];
        actions[a] = { dir: "", fps: 0, loop: !["waving", "jumping"].includes(a) };
      }
      staticMode = true;
      staticFit = Math.min((WIN_W * 0.92) / tex.width, (DISPLAY_H * 1.15) / tex.height);
    } else {
      throw new Error("pet.json 缺 x-actions 且缺 spritesheetPath");
    }
  }
  // 启动加载上次选中的宠物（Rust 持久化，缺省内置宝宝）
  const initialBase = await invoke<string>("current_pet").catch(() => "builtin:/pets/baby");
  await loadPet(initialBase);

  const app = new Application();
  // WebGL 初始化偶发挂起（SecurityError 后 promise 永不结算）——竞速超时保护
  await Promise.race([
    app.init({
      width: WIN_W,
      height: WIN_H,
      backgroundAlpha: 0,
      resolution: window.devicePixelRatio,
      autoDensity: true,
    }),
    new Promise<never>((_, rej) =>
      setTimeout(() => rej(new Error("WebGL 初始化超时（6s）")), 6000),
    ),
  ]);
  document.body.appendChild(app.canvas);

  const sprite = new AnimatedSprite(frames.idle);
  sprite.anchor.set(0.5, 1);
  sprite.position.set(WIN_W / 2, WIN_H - 12);
  sprite.scale = staticMode ? staticFit : DISPLAY_H / (frames.idle[0]?.height || pet.frameHeight || 208);
  sprite.animationSpeed = actions.idle.fps / 60;
  sprite.play();
  app.stage.addChild(sprite);

  const bubbleView = createBubbleView();
  const bubbleC = bubbleView.container;
  const spinner = bubbleView.spinner;
  bubbleC.visible = false;
  app.stage.addChild(bubbleC);
  let bubbleTimer: ReturnType<typeof setTimeout> | null = null;
  let barFade = 0;
  let barTarget = 0;

  let onEnd: (() => void) | null = null;
  // ---- 合成动效引擎（静态图宠物）：程序化变换代替帧动画 ----
  const SYNTH_BASE_Y = WIN_H - 12;
  const SYNTH_BASE_ROT = 0;
  let synthAction = "";
  let synthStart = 0;
  let synthEnd: (() => void) | null = null;
  const SYNTH_DEF: Record<string, { dur: number }> = {
    waving: { dur: 1.4 },
    jumping: { dur: 0.7 },
  };
  const applySynth = (action: string, t: number) => {
    sprite.y = SYNTH_BASE_Y;
    sprite.rotation = SYNTH_BASE_ROT;
    sprite.alpha = 1;
    switch (action) {
      case "idle": sprite.y += Math.sin(t * 2.2) * 2.5; break;                 // 呼吸浮动
      case "jumping": sprite.y += -Math.abs(Math.sin(Math.PI * t / 0.7)) * 30; break; // 弹跳
      case "waving": sprite.rotation = Math.sin(t * 9) * 0.13; break;          // 摆动
      case "failed": sprite.rotation = 0.18; sprite.alpha = 0.75; break;       // 垂头暗淡
      case "waiting": sprite.rotation = Math.sin(t * 1.4) * 0.06; break;       // 缓晃
      case "running-left":
      case "running-right":
      case "running": sprite.rotation = Math.sin(t * 14) * 0.1; break;         // 摇摆跑
      case "review": sprite.y += Math.sin(t * 3) * 1.2; break;                 // 轻点
      case "look-up": sprite.y -= 4; break;
      case "look-down": sprite.y += 4; break;
      case "look-left": sprite.rotation = -0.1; break;
      case "look-right": sprite.rotation = 0.1; break;
    }
  };
  app.ticker.add(() => {
    // 通知条进出场渐变（约 8 帧）
    if (bubbleC.visible) {
      // spinner 旋转（work 态）
      if (spinner.visible) {
        spinner.rotation += app.ticker.deltaMS / 140;
      }
      const step = 0.9 * (app.ticker.deltaMS / 60);
      if (barFade < barTarget) barFade = Math.min(barTarget, barFade + step);
      else if (barFade > barTarget) {
        barFade = Math.max(barTarget, barFade - step);
        if (barFade <= 0) bubbleC.visible = false;
      }
      bubbleC.alpha = barFade;
    }
    if (!staticMode || !synthAction) return;
    const t = (performance.now() - synthStart) / 1000;
    const def = SYNTH_DEF[synthAction];
    applySynth(synthAction, t);
    if (def && t >= def.dur) {
      applySynth("idle", 0);
      synthAction = "idle";
      synthStart = performance.now();
      const cb = synthEnd;
      synthEnd = null;
      cb?.();
    }
  });

  const play = (action: string) => {
    const def = actions[action];
    if (!def || !frames[action]?.length) return;
    sprite.textures = frames[action];
    sprite.scale = staticMode ? staticFit : DISPLAY_H / (frames[action][0]?.height || pet.frameHeight || 208);
    sprite.y = SYNTH_BASE_Y;
    sprite.rotation = SYNTH_BASE_ROT;
    sprite.alpha = 1;
    if (staticMode) {
      sprite.stop();
      synthAction = action;
      synthStart = performance.now();
      return;
    }
    sprite.animationSpeed = def.fps / 60;
    sprite.loop = def.loop;
    sprite.currentFrame = 0;
    sprite.onComplete = () => {
      if (!def.loop && onEnd) {
        const cb = onEnd;
        onEnd = null;
        cb();
      }
    };
    sprite.play();
  };

  const resumeBase = () => {
    if (lastLook && actions[`look-${lastLook}`]) play(`look-${lastLook}`);
    else play("idle");
  };

  // 持续活状态切换（幂等，Codex 派生态语义）
  function setLiveState(st: string, pose: string) {
    if (liveState === st) return;
    liveState = st;
    mode = st === "idle" ? "idle" : "manual";
    if (st === "idle") resumeBase();
    else play(pose);
  }

  const playOnce = (action: string) =>
    new Promise<void>((resolve) => {
      if (!actions[action] || !frames[action]?.length) { resolve(); return; }
      if (staticMode) {
        play(action);
        const dur = (SYNTH_DEF[action]?.dur ?? 1) * 1000;
        setTimeout(resolve, dur);
        return;
      }
      onEnd = resolve;
      play(action);
      if (actions[action].loop) resolve(); // 循环动作由调用方负责收尾
    });

  // ---- 轮播调度 ----
  let idleCycleCount = 0;
  sprite.onFrameChange = () => {
    if (mode === "idle" && sprite.currentFrame === 0) idleCycleCount++;
  };

  async function crawlAbit() {
    const dir = Math.random() < 0.5 ? -1 : 1;
    play(dir < 0 ? "running-left" : "running-right");
    const steps = 22;
    const perStep = (90 * dir) / steps;
    for (let i = 0; i < steps; i++) {
      await invoke("move_by", { dx: perStep, dy: 0 });
      await sleep(90);
    }
  }

  async function doInsert() {
    mode = "insert";
    lastInsert = Date.now();
    const pick = weightedPick(behavior.carousel.weights[moodCategory] ?? behavior.carousel.weights.normal ?? { waving: 1 });
    if (pick === "crawl") await crawlAbit();
    else await playOnce(pick);
    mode = "idle";
    play("idle");
  }

  setInterval(() => {
    if (mode !== "idle" || (!staticMode && idleCycleCount < 1)) return;
    if (Date.now() - lastInsert < behavior.carousel.min_interval_sec * 1000) return;
    if (Math.random() > behavior.carousel.trigger_probability) return;
    idleCycleCount = 0;
    void doInsert();
  }, 1000);

  // ---- 拖拽：原生循环跟随鼠标（drag_begin 后 Rust 逐帧移动窗口，中间零 IPC，丝滑的关键）----
  const canvas = app.canvas;
  let dragging = false;
  let dragMoved = false;
  let lastStatusMsg: StatusMsg | null = null;
  canvas.addEventListener("pointerdown", (e) => {
    if (dragging || e.button !== 0) return;
    dragging = true;
    dragMoved = false;
    mode = "drag";
    canvas.setPointerCapture(e.pointerId);
    void invoke("drag_begin");
  });
  const endDrag = () => {
    if (!dragging) return;
    dragging = false;
    void invoke("drag_end"); // Rust 循环停止并回发 drag-end
  };
  canvas.addEventListener("pointerup", endDrag);
  canvas.addEventListener("pointercancel", endDrag);

  // 拖动方向由原生循环检测并推送（窗口跟随鼠标时，光标在窗口内坐标几乎不变，前端测不到方向）
  await listen<string>("drag-dir", (e) => {
    dragMoved = true;
    if (mode === "drag") play(e.payload === "left" ? "running-left" : "running-right");
  });
  await listen("drag-end", () => {
    if (dragging || mode === "drag") {
      dragging = false;
      mode = "idle";
      resumeBase();
      if (!dragMoved && liveState === "idle") {
        if (lastStatusMsg) showBubbleBar(lastStatusMsg, false);
        mode = "manual";
        void (async () => {
          await playOnce("waving");
          if (mode === "manual") { mode = "idle"; resumeBase(); }
        })();
      }
    }
  });

  // 右键 → 原生菜单（Rust 侧弹出）
  canvas.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    void invoke("popup_menu");
  });

  // ---- M2 事件反应：气泡 + 服务器动作 ----
  function showBubbleBar(m: StatusMsg, persistent = false) {
    bubbleView.show(m);
    bubbleC.position.set(Math.round((WIN_W - BUBBLE_WIDTH) / 2), 2);
    bubbleC.visible = true;
    barTarget = 1;
    if (bubbleTimer) clearTimeout(bubbleTimer);
    if (!persistent) {
      bubbleTimer = setTimeout(() => {
        barTarget = 0; // 渐出（ticker 到 0 后隐藏）
      }, behavior.bubble.duration_sec * 1000);
    }
  }
  function showBubble(text: string, persistent = false) {
    showBubbleBar({ icon: "", title: text, detail: "", tone: "plain" }, persistent);
  }

  // 配置热重载 + 心情推送（Rust → JS）
  await listen<Behavior>("behavior", (e) => {
    behavior = { ...behavior, ...e.payload };
  });
  await listen<{ category: string }>("mood", (e) => {
    moodCategory = e.payload.category;
  });

  await listen<{ action?: string | null; bubble?: string | null; status?: unknown; rich?: unknown; clear?: boolean }>(
    "server-event",
    (e) => {
      if (dragging) return;
      let { action, bubble, status, clear } = e.payload;
      if (clear) {
        barTarget = 0;
        setLiveState("idle", "idle"); // 已读：review/waiting 姿势解除
        return;
      }
      if (status !== undefined) {
        // 诊断魔法字：REDBOX=整窗红色（终极可见性测试）；CLEARBOX=还原透明
        const sv = typeof status === "string" ? status : "";
        // 持久状态条：原地更新，直到新事件替换（Codex 式：图标+标题+正文+状态色）
        if (typeof status === "object" && status !== null) {
          const st = status as { icon: string; title: string; detail: string; tone: string };
          lastStatusMsg = st;
          showBubbleBar(st, true);
          if (dragging) return;
          if (st.tone === "work") setLiveState("running", "running");
          else if (st.tone === "wait") setLiveState("waiting", "waiting");
          else if (st.tone === "bad") setLiveState("failed", "failed");
          else if (st.tone === "good") {
            mode = "manual";
            void (async () => {
              await playOnce("jumping");
              setLiveState("review", "review"); // Codex：完成未读=注视态直到已读
            })();
          }
          return;
        }
        showBubble(String(status), true);
        return;
      }
      if (bubble) showBubble(bubble);
      if ((e.payload as any).rich) showBubbleBar((e.payload as any).rich as StatusMsg, false);
      // 语义回退：当前宠物缺少映射动作时，尝试它的专属同义动作
      // （湖畔小仙：失败→捂耳朵"我不听"，等待→叉腰指前方"该你了"）
      let resolved = action;
      if (resolved && !actions[resolved]) {
        const synonyms: Record<string, string[]> = {
          failed: ["cover-ears"],
          waiting: ["point-forward"],
        };
        for (const alt of synonyms[resolved] ?? []) {
          if (actions[alt]) { resolved = alt; break; }
        }
      }
      if (!resolved || !actions[resolved]) return;
      action = resolved;
    mode = "manual";
    const def = actions[action];
    if (def.loop) {
      play(action);
      setTimeout(() => {
        if (mode === "manual") {
          mode = "idle";
          resumeBase();
        }
      }, 6000);
    } else {
      void (async () => {
        await playOnce(action);
        if (mode === "manual") {
          mode = "idle";
          resumeBase();
        }
      })();
    }
  });

  // 切换宠物（托盘菜单 / CLI deskbuddy pet <id>）
  await listen<{ base: string }>("switch-pet", (e) => {
    void (async () => {
      try {
        await loadPet(e.payload.base);
        if (!frames.idle) throw new Error("该角色缺 idle 动作");
        mode = "manual";
        liveState = "idle";
        lastLook = "";
        synthAction = "";
        idleCycleCount = 0;
        lastInsert = Date.now();
        play("idle");
        await playOnce("waving"); // 换角色打招呼
        mode = "idle";
        resumeBase();
      } catch (err) {
        console.error("切换角色失败", err);
      }
    })();
  });

  // 视线跟随（原生推送四向；仅待机态应用，动作结束后由 resumeBase 恢复）
  await listen<string>("look", (e) => {
    lastLook = e.payload ?? "";
    if (mode === "idle") resumeBase();
  });

  // 悬停检测由原生轮询推送（非激活应用收不到鼠标移动事件，DOM hover 不可用）
  // 行为对齐 Codex 实测：靠近时跳跃，离开后坐下；每次进入都触发
  // 动作与防抖来自 behavior.json 的 hover 段（热重载生效）
  let lastHoverAction = 0;
  let hoverToken = 0;
  await listen("hover-enter", () => {
    if (!behavior.hover.enabled) return;
    if (mode !== "idle") return;
    if (Date.now() - lastHoverAction < behavior.hover.guard_ms) return;
    lastHoverAction = Date.now();
    const token = ++hoverToken;
    const action = behavior.hover.action;
    if (!actions[action]) return;
    mode = "insert";
    void (async () => {
      await playOnce(action);
      if (token === hoverToken && mode === "insert") {
        mode = "idle";
        resumeBase();
      }
    })();
  });
  await listen("hover-leave", () => {
    hoverToken++; // 中断进行中的悬停动作
    if (mode === "insert") {
      mode = "idle";
      resumeBase();
    }
  });

  // 菜单事件（Rust → JS）
  await listen<string>("play-action", (e) => {
    if (dragging) return;
    mode = "manual";
    void (async () => {
      const def = actions[e.payload];
      if (def?.loop) {
        play(e.payload);
        await sleep(4000); // 循环类动作手动播放 4 秒后回 idle
      } else {
        await playOnce(e.payload);
      }
      mode = "idle";
      resumeBase();
    })();
  });

  // 首次启动：打招呼期间锁定动作，避免视线跟随覆盖动画使初始化挂起。
  mode = "manual";
  await sleep(600);
  await playOnce("waving");
  mode = "idle";
  play("idle");
  idleCycleCount = 0;
  sessionStorage.setItem("db-boot-tries", "0");
  (window as any).__dbReady?.(); // 通知看门狗：前端已就绪

  // 渲染心跳：WebGL 上下文假死自检（就绪但空屏的第四种死法），死了自动重载
  const gl = (app.renderer as any).gl as WebGLRenderingContext | undefined;
  const cv = app.canvas as HTMLCanvasElement;
  (window as any).__beacon?.(
    "render-state",
    `type=${(app.renderer as any).type ?? "?"} ctxLost=${gl?.isContextLost?.() ?? "?"} textures=${Object.keys(frames).length} spriteVisible=${sprite.visible} mode=${mode} canvasInDom=${document.body.contains(cv)} canvasSize=${cv.width}x${cv.height} css=${cv.style.width}x${cv.style.height} children=${app.stage.children.length} spriteScale=${sprite.scale?.x ?? "?"} spritePos=${sprite.x},${sprite.y}`,
  );
  // 每帧渲染守护：renderer.render 抛错则上报并自愈重载
  let renderErrs = 0;
  app.ticker.add(() => {
    if (renderErrs > 3) return;
    try {
      (app.renderer as any).render(app.stage);
    } catch (e) {
      renderErrs++;
      (window as any).__beacon?.("render-throw", `${e} (#${renderErrs})`);
      if (renderErrs >= 3) location.reload();
    }
  });
  setInterval(() => {
    try {
      const g = (app.renderer as any).gl as WebGLRenderingContext | undefined;
      if (g?.isContextLost?.()) {
        (window as any).__beacon?.("ctx-lost", "WebGL 上下文丢失，自愈重载");
        location.reload();
      }
    } catch {}
  }, 5000);
}

main().catch((err) => {
  // 渲染进程失败：黑匣子留痕 + 自愈重载（JS 仍存活时比看门狗更快更可靠）
  (window as any).__beacon?.("fatal", err instanceof Error ? err.stack || err.message : String(err));
  console.error(err);
  const tries = Number(sessionStorage.getItem("db-boot-tries") ?? "0");
  if (tries < 3) {
    sessionStorage.setItem("db-boot-tries", String(tries + 1));
    setTimeout(() => location.reload(), 800);
  } else {
    (window as any).__beacon?.("gave-up", "连续3次启动失败，停轮询等看门狗");
  }
});

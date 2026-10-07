import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

// Run the actual desktop entry point with image/IPC/rendering boundaries replaced.
async function desktop(initialBase = 'ext:/static', startTickerImmediately = false, interruptGreeting = false) {
  const listeners = new Map();
  const tickers = [];
  const intervals = [];
  let now = Date.now();
  class Clock extends Date { static now() { return now; } }
  const sprites = [];
  const manifests = {
    static: { displayName: 'Photo', staticImage: 'image.png' },
    animated: { displayName: 'Animated', 'x-actions': {
      idle: { dir: 'idle', fps: 5, loop: true },
      waving: { dir: 'waving', fps: 7, loop: false },
    } },
    'idle-only': { displayName: 'Idle only', 'x-actions': { idle: { dir: 'idle', fps: 5, loop: true } } },
  };
  let ready;
  const boot = new Promise(resolve => { ready = resolve; });
  const errors = [];
  class Image {
    async decode() { this.width = 192; this.height = 208; }
  }
  class AnimatedSprite {
    constructor(textures) { this.textures = textures; sprites.push(this); }
    anchor = { set() {} };
    position = { set() {} };
    scale = 1;
    currentFrame = 0;
    y = 204;
    played = [];
    stop() {}
    play() { this.played.push(this.textures[0].id); if (this.loop === false) { if (interruptGreeting) listeners.get('look')?.({ payload: 'left' }); queueMicrotask(() => this.onComplete?.()); } }
  }
  const canvas = { addEventListener() {}, style: {}, width: 352, height: 216 };
  class Application {
    canvas = canvas;
    ticker = { deltaMS: 16, add(callback) { tickers.push(callback); if (startTickerImmediately) { try { callback(); } catch (error) { errors.push(error); } } } };
    stage = { children: [], addChild(child) { this.children.push(child); } };
    renderer = { render() {} };
    async init() {}
  }
  const invoke = async (name, args) => {
    if (name === 'current_pet') return initialBase;
    if (name !== 'read_pet_file') return;
    const [, role, ...parts] = args.rel.split('/');
    const rel = parts.join('/');
    if (rel === 'pet.json') return 'data:application/json;base64,' + Buffer.from(JSON.stringify(manifests[role])).toString('base64');
    return rel === 'image.png' || rel.endsWith('/00.png') || rel.endsWith('/01.png') ? `${role}/${rel}` : '';
  };
  const imports = {
    './bubble-view': { BUBBLE_WIDTH: 200, createBubbleView: () => ({ container: { visible: false, position: { set() {} } }, spinner: { visible: false }, show() {} }) },
    'pixi.js': { Application, AnimatedSprite, Texture: { from: img => ({ id: img.src, width: 192, height: 208 }) } },
    '@tauri-apps/api/core': { invoke },
    '@tauri-apps/api/event': { listen: async (name, callback) => { listeners.set(name, callback); } },
  };
  const context = vm.createContext({
    require: name => imports[name], exports: {}, Image, fetch, performance, Date: Clock,
    Math: Object.assign(Object.create(Math), { random: () => 0 }),
    window: { devicePixelRatio: 1, __dbReady: ready },
    document: { body: { appendChild() {}, contains: () => true } },
    sessionStorage: { getItem: () => '0', setItem() {} },
    console: { error: (...args) => errors.push(args) },
    setInterval(callback) { intervals.push(callback); }, clearTimeout() {},
    setTimeout(callback, delay) { if (delay < 6000) queueMicrotask(callback); },
    queueMicrotask,
  });
  const source = ts.transpileModule(readFileSync(new URL('../src/main.ts', import.meta.url), 'utf8'), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText;
  vm.runInContext(source, context);
  await Promise.race([boot, new Promise((_, reject) => { const timer = setTimeout(() => reject(new Error('Desktop startup did not finish after greeting was interrupted')), 500); boot.then(() => clearTimeout(timer)); })]);
  return { sprite: sprites[0], tickers, errors, advanceIdleCycle() { now += 21000; sprites[0].onFrameChange?.(); intervals[0](); }, async switchTo(base) {
    listeners.get('switch-pet')({ payload: { base } });
    for (let i = 0; i < 20; i++) await new Promise(setImmediate);
  } };
}

test('switching from a photo to an animated role replaces the desktop texture and plays its frames', async () => {
  const app = await desktop();
  await app.switchTo('ext:/animated');
  assert.equal(app.sprite.textures[0].id, 'animated/idle/00.png');
  assert.equal(app.sprite.textures.length, 2);
  assert.equal(app.sprite.animationSpeed, 5 / 60);
  assert.deepEqual(app.errors, []);
});

test('switching to a photo replaces the previous animation and keeps the photo scale', async () => {
  const app = await desktop('ext:/animated');
  await app.switchTo('ext:/static');
  assert.equal(app.sprite.textures[0].id, 'static/image.png');
  assert.equal(app.sprite.scale, Math.min(352 * 0.92 / 192, 112 * 1.15 / 208));
});

test('a valid role without waving still switches and resumes idle', async () => {
  const app = await desktop('ext:/animated');
  await app.switchTo('ext:/idle-only');
  assert.equal(app.sprite.textures[0].id, 'idle-only/idle/00.png');
  assert.deepEqual(app.errors, []);
});

test('the rendering ticker can run during startup without accessing uninitialized UI', async () => {
  const app = await desktop('ext:/animated', true);
  assert.deepEqual(app.errors, []);
});

test('idle animation retains its cycle callback after greeting and role switches', async () => {
  const app = await desktop('ext:/animated');
  const greetings = () => app.sprite.played.filter(id => id === 'animated/waving/00.png').length;
  const before = greetings();
  app.advanceIdleCycle();
  assert.equal(greetings(), before + 1);
  await app.switchTo('ext:/animated');
  const afterSwitch = greetings();
  app.advanceIdleCycle();
  assert.equal(greetings(), afterSwitch + 1);
});

test('native look events cannot interrupt the startup greeting and stall initialization', async () => {
  const app = await desktop('ext:/animated', false, true);
  assert.deepEqual(app.errors, []);
});

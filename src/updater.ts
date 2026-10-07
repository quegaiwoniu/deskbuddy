export type DownloadEvent = { event: "Started"; data: { contentLength?: number } } | { event: "Progress"; data: { chunkLength: number } } | { event: "Finished" };
export type PendingUpdate = { version: string; body?: string; downloadAndInstall: (onEvent: (event: DownloadEvent) => void) => Promise<void>; close?: () => Promise<void> };
export type UpdateState = { phase: "idle" | "checking" | "current" | "available" | "downloading" | "ready" | "restarting" | "error"; version?: string; notes?: string; downloaded: number; total?: number; error?: string; operation?: "check" | "install" | "restart" };
export class UpdateController {
  state: UpdateState = { phase: "idle", downloaded: 0 };
  constructor(private port: { check: () => Promise<PendingUpdate | null>; restart: () => Promise<void> }, private changed: (state: UpdateState) => void = () => {}) {}
  private pending: PendingUpdate | null = null;
  private installed = false;
  private get busy() { return ["checking", "downloading", "restarting"].includes(this.state.phase); }
  private set(patch: Partial<UpdateState>) {
    this.state = { ...this.state, ...patch };
    this.changed(this.state);
  }
  private fail(operation: "check" | "install" | "restart", error: unknown) {
    this.set({ phase: "error", operation, error: error instanceof Error ? error.message : String(error) });
  }
  async check() {
    if (this.busy || this.installed) return;
    this.set({ phase: "checking", error: undefined, operation: "check", downloaded: 0, total: undefined, version: undefined, notes: undefined });
    try {
      const old = this.pending;
      this.pending = null;
      await old?.close?.();
      this.pending = await this.port.check();
      this.set(this.pending ? { phase: "available", version: this.pending.version, notes: this.pending.body || "此版本未提供更新说明。" } : { phase: "current" });
    } catch (error) { this.fail("check", error); }
  }
  async install() {
    if (this.busy || this.installed || !this.pending) return;
    this.set({ phase: "downloading", error: undefined, operation: "install", downloaded: 0, total: undefined });
    try {
      await this.pending.downloadAndInstall(event => {
        if (event.event === "Started") this.set({ downloaded: 0, total: event.data.contentLength || undefined });
        else if (event.event === "Progress") this.set({ downloaded: this.state.downloaded + event.data.chunkLength });
      });
      this.installed = true;
      this.set({ phase: "ready", error: undefined });
    } catch (error) { this.fail("install", error); }
  }
  async restart() {
    if (this.busy || !this.installed) return;
    this.set({ phase: "restarting", operation: "restart", error: undefined });
    try { await this.port.restart(); }
    catch (error) { this.fail("restart", error); }
  }
}

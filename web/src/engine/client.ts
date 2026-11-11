// Engine client: promise-based requests to engine workers. The pool has a
// main worker holding the build being edited and a background worker for
// slow calculations (node power), which works on a copy of the build so
// edits never wait for it.

import type { WorkerRequest, WorkerResponse } from "./worker";
import { type Download, fetchDownload } from "./downloads";

export class EngineError extends Error {}

class EngineWorker {
  private worker: Worker;
  private next = 1;
  private pending = new Map<number, (r: WorkerResponse["res"]) => void>();

  constructor() {
    this.worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
    this.worker.onmessage = (e: MessageEvent<WorkerResponse>) => {
      const cb = this.pending.get(e.data.id);
      this.pending.delete(e.data.id);
      cb?.(e.data.res);
    };
    this.worker.postMessage({ init: new URL(".", document.baseURI).href } satisfies WorkerRequest);
  }

  private request(req: Record<string, unknown>): Promise<WorkerResponse["res"]> {
    const id = this.next++;
    return new Promise((resolve) => {
      this.pending.set(id, resolve);
      this.worker.postMessage({ id, req } satisfies WorkerRequest);
    });
  }

  // Downloads some request is already fetching: every response lists all
  // queued downloads, so concurrent requests see the same ones
  private claimed = new Set<number>();

  private claim(list?: Download[]): Download[] {
    const mine = (list ?? []).filter((d) => !this.claimed.has(d.id));
    for (const d of mine) this.claimed.add(d.id);
    return mine;
  }

  // A command, and the downloads it started, until none are left (a
  // download's callback may start the next one)
  async call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    const res = await this.request({ cmd, ...args });
    if (res.error !== undefined) throw new EngineError(res.error);
    let downloads = this.claim(res.downloads);
    while (downloads.length) {
      const results = await Promise.all(downloads.map((d) => fetchDownload(d)));
      const next: Download[] = [];
      for (const r of results) {
        const done = await this.request({ cmd: "downloadDone", ...r });
        this.claimed.delete(r.id);
        if (done.error !== undefined) throw new EngineError(done.error);
        next.push(...this.claim(done.downloads));
      }
      downloads = next;
    }
    return res.ok as T;
  }
}

export class Engine {
  readonly main = new EngineWorker();
  private background = new EngineWorker();
  private backgroundXml: string | null = null;

  call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    return this.main.call<T>(cmd, args);
  }

  // Runs a command on the background worker, on the build as it is now
  async background_<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    const { xml } = await this.main.call<{ xml: string }>("saveXml");
    if (xml !== this.backgroundXml) {
      await this.background.call("loadXml", { xml });
      this.backgroundXml = xml;
    }
    return this.background.call<T>(cmd, args);
  }
}

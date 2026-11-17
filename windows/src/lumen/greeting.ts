import { Sound } from "../core/sound";

export const GREETING_END = 3.8;
export const GREETING_W = 640;
export const GREETING_H = 150;

// The GIF loops forever and lasts 41 x 20 ms: after one pass the still PNG takes over.
const CRYSTAL_GIF_MS = 850;

export class Greeting {
  private still = new Image();
  private gif: HTMLImageElement | null = null;
  private startMs = 0;
  private tc = Number.POSITIVE_INFINITY;
  private fired = false;
  private timers: number[] = [];
  onComplete: (() => void) | null = null;

  start() {
    this.startMs = performance.now();
    this.tc = Number.POSITIVE_INFINITY;
    this.fired = false;
    this.cancelTimers();
    this.still.src = "/lumen-crystal.png";
    // A new Image restarts the GIF from its first frame; reduced motion never loads it.
    this.gif = window.matchMedia("(prefers-reduced-motion: reduce)").matches ? null : new Image();
    if (this.gif) this.gif.src = "/lumen-crystal.gif";
    Sound.play("greeting");
    this.timers.push(window.setTimeout(() => this.fire(), GREETING_END * 1000));
  }
  hover() { if (this.tc >= 4.1) this.tc = Number.POSITIVE_INFINITY; }
  interrupt() {
    const t = this.elapsed;
    if (!Number.isFinite(this.tc) || this.tc > t) this.tc = t;
    this.cancelTimers();
    Sound.fadeOut("greeting", 0.2);
  }
  leave() { this.cancelTimers(); Sound.fadeOut("greeting", 0.2); }
  get elapsed() { return (performance.now() - this.startMs) / 1000; }
  get done() { return this.fired; }
  private fire() { if (!this.fired) { this.fired = true; this.cancelTimers(); this.onComplete?.(); } }
  private cancelTimers() { this.timers.forEach((id) => window.clearTimeout(id)); this.timers = []; }
  draw(ctx: CanvasRenderingContext2D) {
    const t = this.elapsed;
    if (!this.fired && t >= GREETING_END && this.tc >= 4.1) this.fire();
    ctx.clearRect(0, 0, GREETING_W, GREETING_H);
    const gif = this.gif && t * 1000 < CRYSTAL_GIF_MS && this.gif.complete && this.gif.naturalWidth > 0 ? this.gif : this.still;
    if (gif.complete && gif.naturalWidth > 0) ctx.drawImage(gif, (GREETING_W - GREETING_H) / 2, 0, GREETING_H, GREETING_H);
  }
}

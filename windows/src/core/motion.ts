/** The visitor asked the system for less movement. Cached, and kept current when the setting flips. */
let reduced = false;
if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
  const q = window.matchMedia("(prefers-reduced-motion: reduce)");
  reduced = q.matches;
  q.addEventListener?.("change", (e) => (reduced = e.matches));
}

export function reducedMotion(): boolean {
  return reduced;
}

/** For tests, and for a setting that overrides the system's. */
export function setReducedMotion(on: boolean) {
  reduced = on;
}

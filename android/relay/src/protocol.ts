// The relay's rules that do not depend on Cloudflare: shared by the Worker (index.ts, room.ts) and by the Node twin
// (tools/dev-relay.ts), so both enforce exactly the same things. Erasable TypeScript only (Node runs it as it is).
// The wire is specified in docs/RELAY_LINK.md.

export const MAX_FRAME = 66_000; // bytes; a v1 line is at most 65,536 plus a header, a tag and padding
export const RATE_PER_SECOND = 30;
export const RATE_BURST = 60;
export const ROOM_ID = /^[A-Za-z0-9_-]{22}$/;
export const B64URL_32 = /^[A-Za-z0-9_-]{43}$/;
export const SUBPROTOCOL = "coucou.v1";
const JOIN_PREFIX = "coucou.join.";

export type Role = "pc" | "phone";
export const otherRole = (r: Role): Role => (r === "pc" ? "phone" : "pc");

/** `/v1/room/<22 chars>` with `?role=pc|phone`, or null (the caller answers 404 and touches nothing). */
export function parseRoomRequest(url: URL): { room: string; role: Role } | null {
  const m = /^\/v1\/room\/([^/]+)$/.exec(url.pathname);
  if (!m || !ROOM_ID.test(m[1])) return null;
  const role = url.searchParams.get("role");
  if (role !== "pc" && role !== "phone") return null;
  return { room: m[1], role };
}

/** The access key from `Authorization: Bearer …`, or null. */
export function bearer(header: string | null): string | null {
  const m = /^Bearer ([A-Za-z0-9_-]{16,128})$/.exec(header ?? "");
  return m ? m[1] : null;
}

/** The join proof from the offered sub-protocols (`coucou.v1, coucou.join.<43 chars>`), or null. */
export function parseJoinProof(header: string | null): string | null {
  if (!header) return null;
  const offered = header.split(",").map((s) => s.trim());
  if (!offered.includes(SUBPROTOCOL)) return null;
  const join = offered.filter((s) => s.startsWith(JOIN_PREFIX));
  if (join.length !== 1) return null;
  const proof = join[0].slice(JOIN_PREFIX.length);
  return B64URL_32.test(proof) ? proof : null;
}

export function fromBase64Url(s: string): Uint8Array {
  const b = atob(s.replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(b, (c) => c.charCodeAt(0));
}

export const toHex = (b: ArrayBuffer | Uint8Array) =>
  [...new Uint8Array(b instanceof Uint8Array ? b : b)].map((x) => x.toString(16).padStart(2, "0")).join("");

async function sha256(data: Uint8Array | string): Promise<Uint8Array> {
  const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
  return new Uint8Array(await crypto.subtle.digest("SHA-256", bytes as BufferSource));
}

/** Constant-time equality of the SHA-256 digests of two strings (their lengths are not revealed). */
export async function sameSecret(a: string, b: string): Promise<boolean> {
  const [x, y] = await Promise.all([sha256(a), sha256(b)]);
  let diff = 0;
  for (let i = 0; i < x.length; i++) diff |= x[i] ^ y[i];
  return diff === 0;
}

/** Is this request's access key the configured one? Fails closed when none is configured (or it is too short to be a key). */
export async function accessKeyOk(configured: string | undefined, header: string | null): Promise<boolean> {
  const sent = bearer(header);
  if (!configured || configured.length < 16 || !sent) return false;
  return sameSecret(sent, configured);
}

/** Tags a socket with the access key it was admitted under (first 8 bytes of its SHA-256, hex). */
export async function accessGeneration(configured: string | undefined): Promise<string> {
  if (!configured) return "";
  return toHex((await sha256(configured)).subarray(0, 8));
}

/** SHA-256 of the 32 raw bytes behind a join proof, as hex: what a room remembers in memory while it has a live socket. */
export async function joinVerifier(proof: string): Promise<string> {
  return toHex(await sha256(fromBase64Url(proof)));
}

/** A token bucket with an injectable clock. */
export class Bucket {
  private tokens: number;
  private last: number;
  private readonly perSecond: number;
  private readonly burst: number;
  private readonly now: () => number;
  // Plain fields, not constructor parameter properties: the Node twin runs this file with Node's type stripping,
  // which only accepts erasable syntax.
  constructor(perSecond: number, burst: number, now: () => number = Date.now) {
    this.perSecond = perSecond;
    this.burst = burst;
    this.now = now;
    this.tokens = burst;
    this.last = now();
  }
  take(): boolean {
    const t = this.now();
    this.tokens = Math.min(this.burst, this.tokens + ((t - this.last) / 1000) * this.perSecond);
    this.last = t;
    if (this.tokens < 1) return false;
    this.tokens -= 1;
    return true;
  }
}

export const PEER_ONLINE = '{"peer":"online"}';
export const PEER_OFFLINE = '{"peer":"offline"}';

// ── Persistent E2EE mailbox (L6.1 / D-054) ──────────────────────────────────────

export const MAILBOX_MAX_BYTES = MAX_FRAME;
export const MAILBOX_MAX_ROOM_BYTES = 16 * 1024 * 1024;
export const MAILBOX_MAX_PENDING_PER_ROLE = 500;
export const MAILBOX_DEFAULT_LIMIT = 50;
export const MAILBOX_MAX_LIMIT = 100;
export const MAILBOX_TTL_MS = 30 * 24 * 60 * 60 * 1000;
export const FREE_PLAN_DAILY_REQUESTS = 100_000;
export const FREE_PLAN_DAILY_DO_WRITES = 100_000;
export const FREE_PLAN_MAX_STORAGE_BYTES = 5 * 1024 * 1024 * 1024;
export const MSG_ID_RE = /^[A-Za-z0-9_-]{8,64}$/;

export type MailboxAction =
  | { room: string; kind: "messages"; method: "POST"; to: Role }
  | { room: string; kind: "messages"; method: "GET"; recipient: Role; after: number; limit: number }
  | { room: string; kind: "ack"; method: "POST"; recipient: Role }
  | { room: string; kind: "revoke"; method: "POST" }
  | { room: string; kind: "stats"; method: "GET" };

export interface FreePlanReport {
  day: string;
  dailyWrites: number;
  dailyWriteLimit: number;
  dailyWritePct: number;
  dailyRequests: number;
  dailyRequestLimit: number;
  enqueuedBytesToday: number;
  roomBytes: number;
  roomBytesLimit: number;
  accountStorageLimitBytes: number;
}

function parseRoleParam(raw: string | null): Role | null {
  return raw === "pc" || raw === "phone" ? raw : null;
}

/** Parses `/v1/mailbox/<22 chars>/(messages|ack|revoke|stats)` or returns `"method"` (405) / `null` (404). */
export function parseMailboxRequest(url: URL, method: string): MailboxAction | "method" | null {
  const m = /^\/v1\/mailbox\/([^/]+)\/(messages|ack|revoke|stats)$/.exec(url.pathname);
  if (!m || !ROOM_ID.test(m[1])) return null;
  const room = m[1];
  const endpoint = m[2];
  if (endpoint === "messages") {
    if (method === "POST") {
      const to = parseRoleParam(url.searchParams.get("to"));
      return to ? { room, kind: "messages", method: "POST", to } : null;
    }
    if (method === "GET") {
      const recipient = parseRoleParam(url.searchParams.get("for"));
      if (!recipient) return null;
      const afterRaw = url.searchParams.get("after");
      const after = afterRaw === null ? 0 : Number(afterRaw);
      if (!Number.isInteger(after) || after < 0) return null;
      const limitRaw = url.searchParams.get("limit");
      const limit = limitRaw === null ? MAILBOX_DEFAULT_LIMIT : Number(limitRaw);
      if (!Number.isInteger(limit) || limit < 1 || limit > MAILBOX_MAX_LIMIT) return null;
      return { room, kind: "messages", method: "GET", recipient, after, limit };
    }
    return "method";
  }
  if (endpoint === "ack") {
    if (method !== "POST") return "method";
    const recipient = parseRoleParam(url.searchParams.get("for"));
    return recipient ? { room, kind: "ack", method: "POST", recipient } : null;
  }
  if (endpoint === "revoke") {
    return method === "POST" ? { room, kind: "revoke", method: "POST" } : "method";
  }
  return method === "GET" ? { room, kind: "stats", method: "GET" } : "method";
}

export function parseMailboxJoinHeader(header: string | null): string | null {
  const value = (header ?? "").trim();
  return B64URL_32.test(value) ? value : null;
}

export function parseMailboxMsgId(header: string | null): string | null | "invalid" {
  if (header === null) return null;
  const trimmed = header.trim();
  if (!trimmed) return null;
  return MSG_ID_RE.test(trimmed) ? trimmed : "invalid";
}

export function utcDayKey(nowMs: number = Date.now()): string {
  return new Date(nowMs).toISOString().slice(0, 10);
}

export function toBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

export function buildFreePlanReport(
  daily: { day: string; writes: number; requests: number; enqueuedBytes: number },
  roomBytes: number,
): FreePlanReport {
  const dailyWritePct = Number(((daily.writes / FREE_PLAN_DAILY_DO_WRITES) * 100).toFixed(4));
  return {
    day: daily.day,
    dailyWrites: daily.writes,
    dailyWriteLimit: FREE_PLAN_DAILY_DO_WRITES,
    dailyWritePct,
    dailyRequests: daily.requests,
    dailyRequestLimit: FREE_PLAN_DAILY_REQUESTS,
    enqueuedBytesToday: daily.enqueuedBytes,
    roomBytes,
    roomBytesLimit: MAILBOX_MAX_ROOM_BYTES,
    accountStorageLimitBytes: FREE_PLAN_MAX_STORAGE_BYTES,
  };
}

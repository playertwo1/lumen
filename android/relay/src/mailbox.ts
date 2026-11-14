// Shared persistent E2EE mailbox handler for both the Cloudflare Worker Room DO (SQLite)
// and the Node twin (`tools/dev-relay.ts`). Never logs, never inspects ciphertext, never sends out.

import {
  MAILBOX_MAX_BYTES,
  MAILBOX_MAX_PENDING_PER_ROLE,
  MAILBOX_MAX_ROOM_BYTES,
  MAILBOX_TTL_MS,
  buildFreePlanReport,
  parseMailboxMsgId,
  toBase64,
  utcDayKey,
  type FreePlanReport,
  type MailboxAction,
  type Role,
} from "./protocol.ts";

export interface MailboxMeta {
  joinVerifier: string;
  revoked: boolean;
  nextSeq: number;
}

export interface StoredMailboxMessage {
  seq: number;
  recipient: Role;
  msgId: string | null;
  sizeBytes: number;
  createdAt: number;
  payload: Uint8Array;
}

export interface DailyCounters {
  day: string;
  writes: number;
  requests: number;
  enqueuedBytes: number;
}

export interface MailboxStorage {
  loadMeta(): MailboxMeta | null;
  initRoom(joinVerifier: string): void;
  markRevoked(joinVerifier: string, day: string): DailyCounters;
  pruneExpired(cutoffMs: number, day: string): number;
  findByMsgId(recipient: Role, msgId: string): { seq: number; id: string | null; createdAt: number } | null;
  pendingStats(recipient: Role): { count: number; totalRoomBytes: number };
  insertMessage(args: {
    recipient: Role;
    msgId: string | null;
    payload: Uint8Array;
    createdAt: number;
    day: string;
  }): { seq: number; storedMessages: number; roomBytes: number; daily: DailyCounters };
  listMessages(
    recipient: Role,
    afterSeq: number,
    limit: number,
  ): { messages: StoredMailboxMessage[]; latestSeq: number; pendingCount: number };
  ackUpto(
    recipient: Role,
    uptoSeq: number,
    day: string,
  ): { deleted: number; remaining: number; roomBytes: number; daily: DailyCounters };
  getStats(day: string): {
    meta: MailboxMeta | null;
    pendingPc: number;
    pendingPhone: number;
    storedBytes: number;
    daily: DailyCounters;
  };
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function empty(status: number): Response {
  return new Response(null, { status });
}

export async function handleMailboxHttp(
  request: Request,
  action: MailboxAction,
  verifier: string,
  storage: MailboxStorage,
  onRevoke?: () => void,
  nowMs: number = Date.now(),
): Promise<Response> {
  const day = utcDayKey(nowMs);
  const meta = storage.loadMeta();

  if (meta) {
    if (meta.revoked) return empty(410);
    if (meta.joinVerifier !== verifier) return empty(403);
  }

  if (action.kind === "revoke") {
    const daily = storage.markRevoked(verifier, day);
    onRevoke?.();
    const freePlan = buildFreePlanReport(daily, 0);
    return json(200, { revoked: true, freePlan });
  }

  if (action.kind === "stats") {
    const stats = storage.getStats(day);
    return json(200, {
      pendingPc: stats.pendingPc,
      pendingPhone: stats.pendingPhone,
      storedBytes: stats.storedBytes,
      revoked: false,
      freePlan: buildFreePlanReport({ ...stats.daily, requests: stats.daily.requests + 1 }, stats.storedBytes),
    });
  }

  if (action.kind === "messages" && action.method === "GET") {
    if (!meta) {
      return json(200, {
        messages: [],
        latestSeq: action.after,
        pendingCount: 0,
        freePlan: buildFreePlanReport({ day, writes: 0, requests: 1, enqueuedBytes: 0 }, 0),
      });
    }
    const listed = storage.listMessages(action.recipient, action.after, action.limit);
    const stats = storage.getStats(day);
    const messages = listed.messages
      .filter((m) => nowMs - m.createdAt <= MAILBOX_TTL_MS)
      .map((m) => ({
        seq: m.seq,
        id: m.msgId,
        createdAt: m.createdAt,
        sizeBytes: m.sizeBytes,
        payloadB64: toBase64(m.payload),
      }));
    return json(200, {
      messages,
      latestSeq: listed.latestSeq,
      pendingCount: listed.pendingCount,
      freePlan: buildFreePlanReport({ ...stats.daily, requests: stats.daily.requests + 1 }, stats.storedBytes),
    });
  }

  if (action.kind === "ack") {
    const raw = await request.text();
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch {
      return empty(400);
    }
    const uptoSeq = typeof parsed === "object" && parsed !== null ? (parsed as { uptoSeq?: unknown }).uptoSeq : undefined;
    if (typeof uptoSeq !== "number" || !Number.isInteger(uptoSeq) || uptoSeq < 0) {
      return empty(400);
    }
    if (!meta) {
      return json(200, {
        ackedUpto: uptoSeq,
        deleted: 0,
        remaining: 0,
        freePlan: buildFreePlanReport({ day, writes: 0, requests: 1, enqueuedBytes: 0 }, 0),
      });
    }
    const res = storage.ackUpto(action.recipient, uptoSeq, day);
    return json(200, {
      ackedUpto: uptoSeq,
      deleted: res.deleted,
      remaining: res.remaining,
      freePlan: buildFreePlanReport(res.daily, res.roomBytes),
    });
  }

  // POST /messages
  const msgId = parseMailboxMsgId(request.headers.get("X-Lumen-Msg-Id"));
  if (msgId === "invalid") return empty(400);

  const declaredLen = request.headers.get("content-length");
  if (declaredLen !== null && Number(declaredLen) > MAILBOX_MAX_BYTES) {
    return empty(413);
  }
  const body = new Uint8Array(await request.arrayBuffer());
  if (body.byteLength === 0) return empty(400);
  if (body.byteLength > MAILBOX_MAX_BYTES) return empty(413);

  if (!meta) {
    storage.initRoom(verifier);
  } else if (msgId) {
    const dup = storage.findByMsgId(action.to, msgId);
    if (dup) {
      const stats = storage.getStats(day);
      return json(200, {
        seq: dup.seq,
        id: dup.id,
        createdAt: dup.createdAt,
        duplicate: true,
        freePlan: buildFreePlanReport(stats.daily, stats.storedBytes),
      });
    }
  }

  const cutoff = nowMs - MAILBOX_TTL_MS;
  let { count, totalRoomBytes } = storage.pendingStats(action.to);
  if (count >= MAILBOX_MAX_PENDING_PER_ROLE || totalRoomBytes + body.byteLength > MAILBOX_MAX_ROOM_BYTES) {
    storage.pruneExpired(cutoff, day);
    ({ count, totalRoomBytes } = storage.pendingStats(action.to));
    if (count >= MAILBOX_MAX_PENDING_PER_ROLE || totalRoomBytes + body.byteLength > MAILBOX_MAX_ROOM_BYTES) {
      return json(507, { error: "mailbox_full" });
    }
  }

  const inserted = storage.insertMessage({
    recipient: action.to,
    msgId,
    payload: body,
    createdAt: nowMs,
    day,
  });
  return json(201, {
    seq: inserted.seq,
    id: msgId,
    createdAt: nowMs,
    duplicate: false,
    storedMessages: inserted.storedMessages,
    storedBytes: inserted.roomBytes,
    freePlan: buildFreePlanReport(inserted.daily, inserted.roomBytes),
  });
}

export class MemoryMailboxStorage implements MailboxStorage {
  private meta: MailboxMeta | null = null;
  private messages: StoredMailboxMessage[] = [];
  private usage = new Map<string, DailyCounters>();

  private bumpDaily(day: string, writesDelta: number, reqDelta: number, bytesDelta: number): DailyCounters {
    const cur = this.usage.get(day) ?? { day, writes: 0, requests: 0, enqueuedBytes: 0 };
    const next: DailyCounters = {
      day,
      writes: cur.writes + writesDelta,
      requests: cur.requests + reqDelta,
      enqueuedBytes: cur.enqueuedBytes + bytesDelta,
    };
    this.usage.set(day, next);
    return next;
  }

  private getDaily(day: string): DailyCounters {
    return this.usage.get(day) ?? { day, writes: 0, requests: 0, enqueuedBytes: 0 };
  }

  private totalBytes(): number {
    return this.messages.reduce((acc, m) => acc + m.sizeBytes, 0);
  }

  loadMeta(): MailboxMeta | null {
    return this.meta ? { ...this.meta } : null;
  }

  initRoom(joinVerifier: string): void {
    if (!this.meta) {
      this.meta = { joinVerifier, revoked: false, nextSeq: 1 };
    }
  }

  markRevoked(joinVerifier: string, day: string): DailyCounters {
    if (!this.meta) {
      this.meta = { joinVerifier, revoked: true, nextSeq: 1 };
    } else {
      this.meta.revoked = true;
    }
    this.messages = [];
    return this.bumpDaily(day, 2, 1, 0);
  }

  pruneExpired(cutoffMs: number, day: string): number {
    const before = this.messages.length;
    this.messages = this.messages.filter((m) => m.createdAt >= cutoffMs);
    const removed = before - this.messages.length;
    if (removed > 0) this.bumpDaily(day, 1, 0, 0);
    return removed;
  }

  findByMsgId(recipient: Role, msgId: string): { seq: number; id: string | null; createdAt: number } | null {
    const found = this.messages.find((m) => m.recipient === recipient && m.msgId === msgId);
    return found ? { seq: found.seq, id: found.msgId, createdAt: found.createdAt } : null;
  }

  pendingStats(recipient: Role): { count: number; totalRoomBytes: number } {
    const count = this.messages.filter((m) => m.recipient === recipient).length;
    return { count, totalRoomBytes: this.totalBytes() };
  }

  insertMessage(args: {
    recipient: Role;
    msgId: string | null;
    payload: Uint8Array;
    createdAt: number;
    day: string;
  }): { seq: number; storedMessages: number; roomBytes: number; daily: DailyCounters } {
    const seq = this.meta!.nextSeq++;
    const copy = new Uint8Array(args.payload);
    this.messages.push({
      seq,
      recipient: args.recipient,
      msgId: args.msgId,
      sizeBytes: copy.byteLength,
      createdAt: args.createdAt,
      payload: copy,
    });
    const daily = this.bumpDaily(args.day, 2, 1, copy.byteLength);
    return {
      seq,
      storedMessages: this.messages.filter((m) => m.recipient === args.recipient).length,
      roomBytes: this.totalBytes(),
      daily,
    };
  }

  listMessages(
    recipient: Role,
    afterSeq: number,
    limit: number,
  ): { messages: StoredMailboxMessage[]; latestSeq: number; pendingCount: number } {
    const allForRole = this.messages.filter((m) => m.recipient === recipient);
    const filtered = allForRole.filter((m) => m.seq > afterSeq).slice(0, limit);
    const latestSeq = filtered.length > 0 ? filtered[filtered.length - 1].seq : afterSeq;
    return {
      messages: filtered,
      latestSeq,
      pendingCount: allForRole.length,
    };
  }

  ackUpto(
    recipient: Role,
    uptoSeq: number,
    day: string,
  ): { deleted: number; remaining: number; roomBytes: number; daily: DailyCounters } {
    const before = this.messages.length;
    this.messages = this.messages.filter((m) => !(m.recipient === recipient && m.seq <= uptoSeq));
    const deleted = before - this.messages.length;
    const remaining = this.messages.filter((m) => m.recipient === recipient).length;
    const daily = deleted > 0 ? this.bumpDaily(day, 2, 1, 0) : this.bumpDaily(day, 0, 1, 0);
    return { deleted, remaining, roomBytes: this.totalBytes(), daily };
  }

  getStats(day: string): {
    meta: MailboxMeta | null;
    pendingPc: number;
    pendingPhone: number;
    storedBytes: number;
    daily: DailyCounters;
  } {
    return {
      meta: this.loadMeta(),
      pendingPc: this.messages.filter((m) => m.recipient === "pc").length,
      pendingPhone: this.messages.filter((m) => m.recipient === "phone").length,
      storedBytes: this.totalBytes(),
      daily: this.getDaily(day),
    };
  }
}

export type { FreePlanReport };

// One room = one Durable Object, named by the room id. At most one live socket per role (pc, phone); frames from one go to
// the other as they are. It never parses, buffers or stores a frame. Hibernation API: an idle room costs nothing.
//
// Everything it remembers lives in the sockets' attachments (hibernation state, not storage): the role, the join verifier
// and the access-key generation the socket was admitted under. When the last socket is gone, nothing is left.

import { DurableObject } from "cloudflare:workers";
import type { Env } from "./index";
import {
  handleMailboxHttp,
  type DailyCounters,
  type MailboxMeta,
  type MailboxStorage,
  type StoredMailboxMessage,
} from "./mailbox";
import {
  Bucket, MAX_FRAME, PEER_OFFLINE, PEER_ONLINE, RATE_BURST, RATE_PER_SECOND, SUBPROTOCOL,
  accessGeneration, joinVerifier, otherRole, parseJoinProof, parseMailboxJoinHeader, parseMailboxRequest,
  parseRoomRequest, type Role,
} from "./protocol";

interface Attachment {
  role: Role;
  verifier: string;
  gen: string;
  replaced?: boolean;
}

const OPEN = 1;

export class Room extends DurableObject<Env> {
  /** Token buckets live in memory only: a room that hibernated has no flood in progress. */
  private readonly buckets = new Map<WebSocket, Bucket>();

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    // The platform answers this itself, without waking the room: the clients' heartbeat.
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair("ping", "pong"));
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname.startsWith("/v1/mailbox/")) {
      const action = parseMailboxRequest(url, request.method);
      if (action === "method") return new Response(null, { status: 405 });
      const proof = parseMailboxJoinHeader(request.headers.get("X-Lumen-Join"));
      if (!action || !proof) return new Response(null, { status: 400 });
      const verifier = await joinVerifier(proof);
      const mailboxStore = new SqliteMailboxStorage(this.ctx.storage.sql);
      return handleMailboxHttp(request, action, verifier, mailboxStore, () => {
        for (const ws of this.ctx.getWebSockets()) {
          try {
            ws.close(1008, "revoked");
          } catch {
            /* already closed */
          }
        }
      });
    }

    const target = parseRoomRequest(url);
    const proof = parseJoinProof(request.headers.get("Sec-WebSocket-Protocol"));
    if (!target || !proof || request.headers.get("Upgrade")?.toLowerCase() !== "websocket") return new Response(null, { status: 400 });

    const verifier = await joinVerifier(proof);
    const persisted = new SqliteMailboxStorage(this.ctx.storage.sql).loadMeta();
    const gen = await accessGeneration(this.env.ACCESS_KEY);
    const live = this.live();
    const wrongProof =
      (live.length > 0 && attachmentOf(live[0]).verifier !== verifier) ||
      (persisted !== null && persisted.joinVerifier !== verifier);

    const pair = new WebSocketPair();
    const [client, server] = [pair[0], pair[1]];
    if (persisted?.revoked) {
      server.accept();
      server.close(1008, "revoked");
      return new Response(null, { status: 101, webSocket: client, headers: { "Sec-WebSocket-Protocol": SUBPROTOCOL } });
    }
    if (wrongProof) {
      // The room is held by someone else's key. Accept, then say why, so the client can show "room is taken".
      server.accept();
      server.close(1008, "join proof");
      return new Response(null, { status: 101, webSocket: client, headers: { "Sec-WebSocket-Protocol": SUBPROTOCOL } });
    }

    // A new socket for a role replaces the old one of that role.
    for (const old of live) {
      const att = attachmentOf(old);
      if (att.role === target.role) {
        old.serializeAttachment({ ...att, replaced: true });
        old.close(1000, "replaced");
      }
    }

    this.ctx.acceptWebSocket(server, [target.role]);
    server.serializeAttachment({ role: target.role, verifier, gen } satisfies Attachment);
    if (this.peerOf(target.role, server)) {
      server.send(PEER_ONLINE);
      this.peerOf(target.role, server)?.send(PEER_ONLINE);
    }
    return new Response(null, { status: 101, webSocket: client, headers: { "Sec-WebSocket-Protocol": SUBPROTOCOL } });
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    const att = attachmentOf(ws);
    // The key the socket was admitted under must still be the configured one (rotation closes live sockets here).
    if (!att.gen || att.gen !== (await accessGeneration(this.env.ACCESS_KEY))) {
      ws.close(1008, "access key changed");
      return;
    }
    // "ping" never gets here (the platform answers it). Nothing else may be text.
    if (typeof message === "string") {
      ws.close(1003, "binary only");
      return;
    }
    if (message.byteLength > MAX_FRAME) {
      ws.close(1009, "too big");
      return;
    }
    let bucket = this.buckets.get(ws);
    if (!bucket) this.buckets.set(ws, (bucket = new Bucket(RATE_PER_SECOND, RATE_BURST)));
    if (!bucket.take()) {
      ws.close(1008, "rate");
      return;
    }
    const peer = this.peerOf(att.role, ws);
    if (!peer) {
      ws.send(PEER_OFFLINE); // nothing is buffered: the frame is dropped
      return;
    }
    peer.send(message);
  }

  async webSocketClose(ws: WebSocket, code: number, _reason: string, _wasClean: boolean): Promise<void> {
    this.gone(ws);
    try {
      ws.close(code >= 1000 && code <= 1011 && code !== 1005 && code !== 1006 ? code : 1000, "");
    } catch {
      /* already closed */
    }
  }

  async webSocketError(ws: WebSocket): Promise<void> {
    this.gone(ws);
  }

  // ── helpers ────────────────────────────────────────────────────────────────────

  private live(): WebSocket[] {
    return this.ctx.getWebSockets().filter((w) => w.readyState === OPEN && !attachmentOf(w).replaced);
  }

  private peerOf(role: Role, except: WebSocket): WebSocket | undefined {
    return this.ctx.getWebSockets(otherRole(role)).find((w) => w !== except && w.readyState === OPEN && !attachmentOf(w).replaced);
  }

  /** A socket left: forget its bucket, and tell the other side unless the same role is still here (a replacement). */
  private gone(ws: WebSocket): void {
    this.buckets.delete(ws);
    const att = attachmentOf(ws);
    if (att.replaced) return;
    const sameRoleStillHere = this.ctx.getWebSockets(att.role).some((w) => w !== ws && w.readyState === OPEN && !attachmentOf(w).replaced);
    if (sameRoleStillHere) return;
    this.peerOf(att.role, ws)?.send(PEER_OFFLINE);
  }
}

function attachmentOf(ws: WebSocket): Attachment {
  return (ws.deserializeAttachment() ?? { role: "pc", verifier: "", gen: "" }) as Attachment;
}

class SqliteMailboxStorage implements MailboxStorage {
  private readonly sql: SqlStorage;

  constructor(sql: SqlStorage) {
    this.sql = sql;
  }

  private hasTables(): boolean {
    const rows = this.sql.exec<{ ok: number }>(
      "SELECT 1 AS ok FROM sqlite_master WHERE type = 'table' AND name = 'mailbox_meta'",
    ).toArray();
    return rows.length > 0;
  }

  private ensureTables(): void {
    this.sql.exec(`
      CREATE TABLE IF NOT EXISTS mailbox_meta (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        join_verifier TEXT NOT NULL,
        revoked INTEGER NOT NULL DEFAULT 0,
        next_seq INTEGER NOT NULL DEFAULT 1
      );
      CREATE TABLE IF NOT EXISTS mailbox_messages (
        seq INTEGER PRIMARY KEY,
        recipient TEXT NOT NULL,
        msg_id TEXT,
        size_bytes INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        payload BLOB NOT NULL
      );
      CREATE UNIQUE INDEX IF NOT EXISTS idx_mailbox_msg_dedup
        ON mailbox_messages(recipient, msg_id)
        WHERE msg_id IS NOT NULL;
      CREATE TABLE IF NOT EXISTS mailbox_daily_usage (
        day TEXT PRIMARY KEY,
        writes INTEGER NOT NULL DEFAULT 0,
        requests INTEGER NOT NULL DEFAULT 0,
        enqueued_bytes INTEGER NOT NULL DEFAULT 0
      );
    `);
  }

  private bumpDaily(day: string, writesDelta: number, reqDelta: number, bytesDelta: number): DailyCounters {
    if (writesDelta > 0 || reqDelta > 0 || bytesDelta > 0) {
      this.sql.exec(
        `INSERT INTO mailbox_daily_usage (day, writes, requests, enqueued_bytes)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(day) DO UPDATE SET
           writes = writes + excluded.writes,
           requests = requests + excluded.requests,
           enqueued_bytes = enqueued_bytes + excluded.enqueued_bytes`,
        day,
        writesDelta,
        reqDelta,
        bytesDelta,
      );
    }
    return this.getDaily(day);
  }

  private getDaily(day: string): DailyCounters {
    const row = this.sql.exec<{ day: string; writes: number; requests: number; enqueued_bytes: number }>(
      "SELECT day, writes, requests, enqueued_bytes FROM mailbox_daily_usage WHERE day = ?",
      day,
    ).toArray()[0];
    return row
      ? { day: row.day, writes: row.writes, requests: row.requests, enqueuedBytes: row.enqueued_bytes }
      : { day, writes: 0, requests: 0, enqueuedBytes: 0 };
  }

  loadMeta(): MailboxMeta | null {
    if (!this.hasTables()) return null;
    const row = this.sql.exec<{ join_verifier: string; revoked: number; next_seq: number }>(
      "SELECT join_verifier, revoked, next_seq FROM mailbox_meta WHERE id = 1",
    ).toArray()[0];
    if (!row) return null;
    return { joinVerifier: row.join_verifier, revoked: row.revoked !== 0, nextSeq: row.next_seq };
  }

  initRoom(joinVerifier: string): void {
    this.ensureTables();
    this.sql.exec(
      "INSERT OR IGNORE INTO mailbox_meta (id, join_verifier, revoked, next_seq) VALUES (1, ?, 0, 1)",
      joinVerifier,
    );
  }

  markRevoked(joinVerifier: string, day: string): DailyCounters {
    this.ensureTables();
    this.sql.exec(
      `INSERT INTO mailbox_meta (id, join_verifier, revoked, next_seq) VALUES (1, ?, 1, 1)
       ON CONFLICT(id) DO UPDATE SET revoked = 1`,
      joinVerifier,
    );
    this.sql.exec("DELETE FROM mailbox_messages");
    return this.bumpDaily(day, 2, 1, 0);
  }

  pruneExpired(cutoffMs: number, day: string): number {
    if (!this.hasTables()) return 0;
    const before = this.sql.exec<{ c: number }>("SELECT COUNT(*) AS c FROM mailbox_messages WHERE created_at < ?", cutoffMs).one().c;
    if (before > 0) {
      this.sql.exec("DELETE FROM mailbox_messages WHERE created_at < ?", cutoffMs);
      this.bumpDaily(day, 1, 0, 0);
    }
    return before;
  }

  findByMsgId(recipient: Role, msgId: string): { seq: number; id: string | null; createdAt: number } | null {
    if (!this.hasTables()) return null;
    const row = this.sql.exec<{ seq: number; msg_id: string | null; created_at: number }>(
      "SELECT seq, msg_id, created_at FROM mailbox_messages WHERE recipient = ? AND msg_id = ? LIMIT 1",
      recipient,
      msgId,
    ).toArray()[0];
    return row ? { seq: row.seq, id: row.msg_id, createdAt: row.created_at } : null;
  }

  pendingStats(recipient: Role): { count: number; totalRoomBytes: number } {
    if (!this.hasTables()) return { count: 0, totalRoomBytes: 0 };
    const count = this.sql.exec<{ c: number }>("SELECT COUNT(*) AS c FROM mailbox_messages WHERE recipient = ?", recipient).one().c;
    const totalRoomBytes = this.sql.exec<{ b: number | null }>("SELECT COALESCE(SUM(size_bytes), 0) AS b FROM mailbox_messages").one().b ?? 0;
    return { count, totalRoomBytes };
  }

  insertMessage(args: {
    recipient: Role;
    msgId: string | null;
    payload: Uint8Array;
    createdAt: number;
    day: string;
  }): { seq: number; storedMessages: number; roomBytes: number; daily: DailyCounters } {
    const seq = this.sql.exec<{ next_seq: number }>("SELECT next_seq FROM mailbox_meta WHERE id = 1").one().next_seq;
    this.sql.exec("UPDATE mailbox_meta SET next_seq = ? WHERE id = 1", seq + 1);
    const bytes = args.payload.buffer.slice(args.payload.byteOffset, args.payload.byteOffset + args.payload.byteLength);
    this.sql.exec(
      "INSERT INTO mailbox_messages (seq, recipient, msg_id, size_bytes, created_at, payload) VALUES (?, ?, ?, ?, ?, ?)",
      seq,
      args.recipient,
      args.msgId,
      args.payload.byteLength,
      args.createdAt,
      bytes,
    );
    const daily = this.bumpDaily(args.day, 2, 1, args.payload.byteLength);
    const stats = this.pendingStats(args.recipient);
    return { seq, storedMessages: stats.count, roomBytes: stats.totalRoomBytes, daily };
  }

  listMessages(
    recipient: Role,
    afterSeq: number,
    limit: number,
  ): { messages: StoredMailboxMessage[]; latestSeq: number; pendingCount: number } {
    if (!this.hasTables()) return { messages: [], latestSeq: afterSeq, pendingCount: 0 };
    const pendingCount = this.sql.exec<{ c: number }>("SELECT COUNT(*) AS c FROM mailbox_messages WHERE recipient = ?", recipient).one().c;
    const rows = this.sql.exec<{
      seq: number;
      recipient: string;
      msg_id: string | null;
      size_bytes: number;
      created_at: number;
      payload: ArrayBuffer;
    }>(
      "SELECT seq, recipient, msg_id, size_bytes, created_at, payload FROM mailbox_messages WHERE recipient = ? AND seq > ? ORDER BY seq ASC LIMIT ?",
      recipient,
      afterSeq,
      limit,
    ).toArray();
    const messages: StoredMailboxMessage[] = rows.map((r) => ({
      seq: r.seq,
      recipient: r.recipient === "pc" ? "pc" : "phone",
      msgId: r.msg_id,
      sizeBytes: r.size_bytes,
      createdAt: r.created_at,
      payload: new Uint8Array(r.payload),
    }));
    const latestSeq = messages.length > 0 ? messages[messages.length - 1].seq : afterSeq;
    return { messages, latestSeq, pendingCount };
  }

  ackUpto(
    recipient: Role,
    uptoSeq: number,
    day: string,
  ): { deleted: number; remaining: number; roomBytes: number; daily: DailyCounters } {
    if (!this.hasTables()) return { deleted: 0, remaining: 0, roomBytes: 0, daily: { day, writes: 0, requests: 1, enqueuedBytes: 0 } };
    const deleted = this.sql.exec<{ c: number }>(
      "SELECT COUNT(*) AS c FROM mailbox_messages WHERE recipient = ? AND seq <= ?",
      recipient,
      uptoSeq,
    ).one().c;
    if (deleted > 0) {
      this.sql.exec("DELETE FROM mailbox_messages WHERE recipient = ? AND seq <= ?", recipient, uptoSeq);
    }
    const daily = deleted > 0 ? this.bumpDaily(day, 2, 1, 0) : this.bumpDaily(day, 0, 1, 0);
    const stats = this.pendingStats(recipient);
    return { deleted, remaining: stats.count, roomBytes: stats.totalRoomBytes, daily };
  }

  getStats(day: string): {
    meta: MailboxMeta | null;
    pendingPc: number;
    pendingPhone: number;
    storedBytes: number;
    daily: DailyCounters;
  } {
    if (!this.hasTables()) {
      return {
        meta: null,
        pendingPc: 0,
        pendingPhone: 0,
        storedBytes: 0,
        daily: { day, writes: 0, requests: 0, enqueuedBytes: 0 },
      };
    }
    const meta = this.loadMeta();
    const pendingPc = this.sql.exec<{ c: number }>("SELECT COUNT(*) AS c FROM mailbox_messages WHERE recipient = 'pc'").one().c;
    const pendingPhone = this.sql.exec<{ c: number }>("SELECT COUNT(*) AS c FROM mailbox_messages WHERE recipient = 'phone'").one().c;
    const storedBytes = this.sql.exec<{ b: number | null }>("SELECT COALESCE(SUM(size_bytes), 0) AS b FROM mailbox_messages").one().b ?? 0;
    return { meta, pendingPc, pendingPhone, storedBytes, daily: this.getDaily(day) };
  }
}

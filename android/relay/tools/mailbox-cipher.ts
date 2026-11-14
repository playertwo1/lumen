// Cifra da caixa de correio `coucou-mailbox/v1` (segundamente/docs/lumen/CONTRATO-SYNC.md, seção 4).
// Implementação de referência em Node, usada só para gerar e conferir test-vectors-mailbox.json.
// O Worker não decifra nada. Erasable TypeScript only (roda com `node`, como tools/dev-relay.ts).

import { createCipheriv, createDecipheriv, hkdfSync } from "node:crypto";

export const RELAY_LABEL = "coucou-relay/v1";
export const MAILBOX_LABEL = "coucou-mailbox/v1";
export const MAILBOX_VER = 1;
export const MAILBOX_TO = { pc: 1, phone: 2 } as const;
export type Role = keyof typeof MAILBOX_TO;
export const MIN_BODY = 1 + 12 + 128 + 16;
export const MAX_JSON = 65536;

export type Refusal = "too_short" | "bad_version" | "bad_tag" | "bad_length" | "bad_padding" | "bad_json";
export type Opened = { ok: true; json: string } | { ok: false; reason: Refusal };

export function deriveMailboxKey(k: Uint8Array): Buffer {
  return Buffer.from(hkdfSync("sha256", k, Buffer.alloc(0), `${RELAY_LABEL} mailbox`, 32));
}

export function aad(room: string, to: Role): Buffer {
  return Buffer.concat([Buffer.from(MAILBOX_LABEL, "ascii"), Buffer.from(room, "ascii"), Buffer.from([MAILBOX_TO[to], MAILBOX_VER])]);
}

// u32 big-endian (tamanho do JSON) ‖ JSON ‖ zeros até múltiplo de 128 bytes (mínimo 128), como no canal ao vivo.
export function pad(json: string): Buffer {
  const text = Buffer.from(json, "utf8");
  const len = Buffer.alloc(4);
  len.writeUInt32BE(text.length);
  const raw = Buffer.concat([len, text]);
  const total = Math.max(128, Math.ceil(raw.length / 128) * 128);
  return Buffer.concat([raw, Buffer.alloc(total - raw.length)]);
}

// Corpo do POST: ver ‖ nonce ‖ ciphertext ‖ tag.
export function sealPlaintext(mailboxKey: Buffer, room: string, to: Role, nonce: Buffer, plaintext: Buffer): Buffer {
  const c = createCipheriv("aes-256-gcm", mailboxKey, nonce, { authTagLength: 16 });
  c.setAAD(aad(room, to));
  const ct = Buffer.concat([c.update(plaintext), c.final()]);
  return Buffer.concat([Buffer.from([MAILBOX_VER]), nonce, ct, c.getAuthTag()]);
}

export function sealJson(mailboxKey: Buffer, room: string, to: Role, nonce: Buffer, json: string): Buffer {
  return sealPlaintext(mailboxKey, room, to, nonce, pad(json));
}

// Recusa com um motivo fixo; quem chama não deve registrar o conteúdo.
export function open(mailboxKey: Buffer, room: string, to: Role, body: Buffer): Opened {
  const fail = (reason: Refusal): Opened => ({ ok: false, reason });
  if (body.length < MIN_BODY) return fail("too_short");
  if (body[0] !== MAILBOX_VER) return fail("bad_version");
  const nonce = body.subarray(1, 13);
  const ct = body.subarray(13, body.length - 16);
  const tag = body.subarray(body.length - 16);
  const d = createDecipheriv("aes-256-gcm", mailboxKey, nonce, { authTagLength: 16 });
  d.setAAD(aad(room, to));
  d.setAuthTag(tag);
  let plain: Buffer;
  try {
    plain = Buffer.concat([d.update(ct), d.final()]);
  } catch {
    return fail("bad_tag");
  }
  const declared = plain.readUInt32BE(0);
  if (declared > MAX_JSON || 4 + declared > plain.length) return fail("bad_length");
  const end = 4 + declared;
  if (plain.subarray(end).some((b) => b !== 0)) return fail("bad_padding");
  const text = plain.subarray(4, end).toString("utf8");
  try {
    JSON.parse(text);
  } catch {
    return fail("bad_json");
  }
  return { ok: true, json: text };
}

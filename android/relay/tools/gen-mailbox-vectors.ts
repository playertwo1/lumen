#!/usr/bin/env node
// Gera android/relay/test-vectors-mailbox.json: vetores sintéticos da cifra da caixa de correio
// (segundamente/docs/lumen/CONTRATO-SYNC.md, seção 4), com as mesmas entradas key/room de test-vectors.json.
// Os nonces são fixos só para estes vetores; em produção o nonce é aleatório (CSPRNG).
//
//   node android/relay/tools/gen-mailbox-vectors.ts           escreve test-vectors-mailbox.json
//   node android/relay/tools/gen-mailbox-vectors.ts --check   falha se o arquivo não for o que este script produz (CI)

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { MAILBOX_TO, aad, deriveMailboxKey, open, pad, sealJson, sealPlaintext, type Role } from "./mailbox-cipher.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, "..", "test-vectors-mailbox.json");
const shared = JSON.parse(readFileSync(join(HERE, "..", "test-vectors.json"), "utf8"));

const hex = (b: Buffer) => b.toString("hex");
const seq = (start: number, n: number) => Buffer.from(Array.from({ length: n }, (_, i) => (start + i) & 0xff));

// ── entradas ────────────────────────────────────────────────────────────────────
const K = Buffer.from(shared.inputs.key, "hex");
const room: string = shared.inputs.room;
const kMailbox = deriveMailboxKey(K);
const otherRoom = seq(0xb0, 16).toString("base64url");

// Envelopes da seção 8 do contrato, copiados sem alteração (JSON compacto).
const env81 = {
  contract_version: 1, mutation_id: "mut-syn-task-001", device_id: "dev-syn-pc-01", origin_role: "pc", entity_type: "task",
  entity_id: "task-syn-001", base_version: 0, new_version: 1, updated_at: "2026-11-14T11:30:00Z",
  payload: {
    id: "task-syn-001", text: "Revisar pauta da reunião semanal", due: "2026-11-15 09:00", due_date: "2026-11-15", due_time: "09:00",
    time_zone: "America/Sao_Paulo", alert_minutes: 20, priority: "high", subtasks: [{ id: "sub-syn-1", text: "Separar tópicos", done: false }],
    recurrence: null, series_id: null, done: false, created_at: "2026-11-14T11:30:00Z", done_at: null, snoozed_until: null,
    source: null, source_stale: false, version: 1, deleted_at: null,
  },
};
const env82 = {
  contract_version: 1, mutation_id: "mut-syn-task-002", device_id: "dev-syn-phone-01", origin_role: "phone", entity_type: "task",
  entity_id: "task-syn-001", base_version: 1, new_version: 2, updated_at: "2026-11-15T08:45:00Z",
  payload: {
    id: "task-syn-001", text: "Revisar pauta da reunião semanal", due: "2026-11-15 09:00", due_date: "2026-11-15", due_time: "09:00",
    time_zone: "America/Sao_Paulo", alert_minutes: 20, priority: "high", subtasks: [{ id: "sub-syn-1", text: "Separar tópicos", done: true }],
    recurrence: null, series_id: null, done: true, created_at: "2026-11-14T11:30:00Z", done_at: "2026-11-15T08:45:00Z", snoozed_until: null,
    source: null, source_stale: false, version: 2, deleted_at: null,
  },
};
const env83 = {
  contract_version: 1, mutation_id: "mut-syn-task-003", device_id: "dev-syn-pc-01", origin_role: "pc", entity_type: "task",
  entity_id: "task-syn-002", base_version: 3, new_version: 4, updated_at: "2026-11-16T10:00:00Z",
  payload: {
    id: "task-syn-002", text: "Regar as plantas da varanda", due: "", due_date: null, due_time: null, time_zone: "America/Sao_Paulo",
    alert_minutes: 20, priority: "low", subtasks: [], recurrence: { frequency: "weekly", weekdays: [2, 5], day_of_month: null },
    series_id: "series-syn-002", done: false, created_at: "2026-11-10T09:00:00Z", done_at: null, snoozed_until: null,
    version: 4, deleted_at: "2026-11-16T10:00:00Z",
  },
};

// ── mensagens que devem ser aceitas ─────────────────────────────────────────────
const cases: { to: Role; json: string; nonce: Buffer }[] = [
  { to: "phone", json: JSON.stringify(env81), nonce: seq(0xc0, 12) },
  { to: "pc", json: JSON.stringify(env82), nonce: seq(0xd0, 12) },
  { to: "phone", json: JSON.stringify(env83), nonce: seq(0xe0, 12) },
];
const messages = cases.map(({ to, json, nonce }) => {
  const plaintext = pad(json);
  const body = sealPlaintext(kMailbox, room, to, nonce, plaintext);
  return { to, nonce: hex(nonce), aad: hex(aad(room, to)), plaintextPadded: hex(plaintext), body: hex(body), json };
});

// ── corpos que devem ser recusados (to/room são os do receptor) ─────────────────
const good = Buffer.from(messages[0].body, "hex");
const flip = (b: Buffer, at: number) => {
  const c = Buffer.from(b);
  c[at] ^= 0x01;
  return c;
};
const dirty = pad(JSON.stringify(env81));
dirty[dirty.length - 1] = 0x01; // um byte não zero no padding, com tag válida
const declaredBeyondText = Buffer.alloc(128);
declaredBeyondText.writeUInt32BE(500, 0);
declaredBeyondText.write("{}", 4);
const withVer2 = Buffer.from(good);
withVer2[0] = 2;

const mustRefuse = [
  { name: "a bit flipped in the ciphertext", expect: "bad_tag", to: "phone", room, body: hex(flip(good, 13)) },
  { name: "a bit flipped in the tag", expect: "bad_tag", to: "phone", room, body: hex(flip(good, good.length - 1)) },
  { name: "ver = 2", expect: "bad_version", to: "phone", room, body: hex(withVer2) },
  { name: "a phone message opened as pc (AAD with the other to)", expect: "bad_tag", to: "pc", room, body: messages[0].body },
  { name: "another room", expect: "bad_tag", to: "phone", room: otherRoom, body: messages[0].body },
  { name: "truncated (156 bytes, one short of the minimum)", expect: "too_short", to: "phone", room, body: hex(good.subarray(0, 156)) },
  { name: "a non-zero byte in the padding", expect: "bad_padding", to: "phone", room, body: hex(sealPlaintext(kMailbox, room, "phone", seq(0xf0, 12), dirty)) },
  { name: "declared size (500) larger than the text", expect: "bad_length", to: "phone", room, body: hex(sealPlaintext(kMailbox, room, "phone", seq(0xf1, 12), declaredBeyondText)) },
  { name: "valid tag, JSON invalid", expect: "bad_json", to: "phone", room, body: hex(sealJson(kMailbox, room, "phone", seq(0xf2, 12), "{nao e json")) },
];

// ── self-checks: cada mensagem abre com o JSON esperado; cada recusa recusa pelo motivo certo ──
for (const m of messages) {
  const opened = open(kMailbox, room, m.to, Buffer.from(m.body, "hex"));
  if (!opened.ok || opened.json !== m.json) throw new Error(`message to ${m.to} does not open to its json`);
}
for (const n of mustRefuse) {
  const opened = open(kMailbox, n.room, n.to as Role, Buffer.from(n.body, "hex"));
  if (opened.ok || opened.reason !== n.expect) throw new Error(`refusal vector not refused as ${n.expect}: ${n.name}`);
}

const out = {
  _about: "Generated by android/relay/tools/gen-mailbox-vectors.ts from test-vectors.json inputs. Do not edit by hand: change the script and regenerate. All byte strings are lowercase hex (bodies included). Nonces are fixed here only so the vectors are deterministic; in production the nonce is random (CSPRNG). Spec: segundamente/docs/lumen/CONTRATO-SYNC.md section 4.",
  version: 1,
  label: "coucou-mailbox/v1",
  kdfInfo: "coucou-relay/v1 mailbox",
  to: MAILBOX_TO,
  inputs: { key: shared.inputs.key, room },
  kMailbox: hex(kMailbox),
  messages,
  mustRefuse,
};

const text = JSON.stringify(out, null, 2) + "\n";
if (process.argv.includes("--check")) {
  const have = readFileSync(OUT, "utf8");
  if (have !== text) {
    console.error("android/relay/test-vectors-mailbox.json is not what gen-mailbox-vectors.ts produces. Run: node android/relay/tools/gen-mailbox-vectors.ts");
    process.exit(1);
  }
  console.log("mailbox test vectors are up to date");
} else {
  writeFileSync(OUT, text);
  console.log(`wrote ${OUT}`);
}

import { test } from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import ts from "typescript";
import { installFakeDom } from "./fakedom.mjs";
import { calls, emit, internals, sent } from "./tauri.mjs";
import { FlowBridge } from "../src/core/flow.ts";

const { root } = installFakeDom();
// The page imports the real animated hero, whose parameter property needs a
// TypeScript transform rather than Node's built-in type stripping.
registerHooks({ load(url, context, nextLoad) {
  const loaded = nextLoad(url, context);
  if (!url.endsWith("/src/hub/hero.ts")) return loaded;
  return { ...loaded, format: "module", source: ts.transpileModule(String(loaded.source), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
  }).outputText };
} });
document.addEventListener = () => {};
window.addEventListener ??= () => {};
document.documentElement = { lang: "en" };
document.visibilityState = "hidden";
globalThis.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
Object.getPrototypeOf(root()).prepend = function (...nodes) {
  for (const node of nodes) node.parentNode = this;
  this.childNodes.unshift(...nodes);
};
const { notesPage, resolvePersonaAvatar, formatImportedPersonaFiles } = await import("../src/hub/pages/notes.ts");
const flush = () => new Promise(resolve => setTimeout(resolve, 0));
const originalInvoke = internals.invoke;

async function recordingPage(t) {
  t.mock.method(globalThis, "setInterval", () => 1);
  t.mock.method(globalThis, "clearInterval", () => {});
  const note = { id: "synthetic", title: "Synthetic call", start: Date.now() - 10000,
    status: "ready", speakers: [], lines: [{ start: 0, speaker: "me", text: "Test" }],
    summary: "", keyPoints: [], todos: [] };
  t.mock.method(internals, "invoke", async (cmd, args) => {
    if (cmd === "notes_state") return { notes: [note], recording: null, working: [] };
    return originalInvoke(cmd, args);
  });
  const page = root();
  const ctx = { hero: () => root(), status: { models: { parakeet: { state: "ready" } } } };
  await notesPage(page, ctx);
  t.after(() => ctx.onLeave());
  return { page };
}

async function exportPage(t) {
  const fixture = await recordingPage(t);
  return {
    ...fixture,
    exportButton: () => fixture.page.find("button").find(b => b.textContent === "Export as Markdown…"),
    detail: () => fixture.page.querySelector(".hub-note-detail"),
  };
}

test("a pending export opens only one dialog and cancellation reports no saved file", async t => {
  const fixture = await exportPage(t);
  let complete, requests = 0;
  t.mock.method(FlowBridge, "exportNote", () => {
    requests++;
    return new Promise(resolve => { complete = resolve; });
  });
  const button = fixture.exportButton();
  button.fire("click");
  assert.equal(button.disabled, true);
  button.fire("click");
  assert.equal(requests, 1);
  complete(null);
  await flush();
  assert.equal(fixture.exportButton().disabled, false);
  assert.doesNotMatch(fixture.detail().textContent, /Saved as/);
});

test("an export failure is visible and a successful retry reports the selected path", async t => {
  const fixture = await exportPage(t);
  let fail = true;
  t.mock.method(FlowBridge, "exportNote", async () => {
    if (fail) throw new Error("Destination is not writable");
    return "C:\\chosen\\Synthetic call.md";
  });
  fixture.exportButton().fire("click");
  await flush();
  assert.match(fixture.detail().textContent, /Destination is not writable/);
  assert.equal(fixture.exportButton().disabled, false);
  fail = false;
  fixture.exportButton().fire("click");
  await flush();
  assert.doesNotMatch(fixture.detail().textContent, /Destination is not writable/);
  assert.match(fixture.detail().textContent, /Saved as C:\\chosen\\Synthetic call\.md/);
});

test("the notes search: every word, case and accents ignored, in any part of the note", async () => {
  const { noteMatches } = await import("../src/core/flow.ts");
  const note = {
    title: "Call · 11-09 14:00", summary: "Orçamento do projeto", keyPoints: ["Prazo em dezembro"],
    todos: [{ id: "1", text: "Enviar proposta", done: false }],
    speakers: [{ id: "a", name: "Marina", color: 0, isMe: false }],
    lines: [{ id: "l", speaker: "a", start: 0, end: 1, text: "Vamos fechar o contrato" }],
    comments: [{ id: "c1", characterId: "clara", characterName: "Clara", authorKind: "persona", content: "Priorize o cronograma técnico.", createdAt: 1 }],
  };
  for (const q of ["", "  ", "orcamento", "MARINA contrato", "dezembro proposta", "call 11-09", "clara cronograma"]) assert.ok(noteMatches(note, q), q);
  for (const q of ["marina budget", "zzz"]) assert.ok(!noteMatches(note, q), q);
});

test("Anotador permite criar nota em texto e enviar comentário para personas", async t => {
  const fixture = await recordingPage(t);
  let createdArgs = null;
  let commentArgs = null;
  t.mock.method(FlowBridge, "createTextNote", async (title, text) => {
    createdArgs = { title, text };
    return {
      id: "n-text",
      title: title || "Ideia sintética",
      titleEdited: Boolean(title),
      start: Date.now(),
      status: "ready",
      speakers: [],
      lines: [{ id: "l1", speaker: "me", start: 0, end: 0, text }],
      keyPoints: [],
      todos: [],
      comments: [],
    };
  });
  t.mock.method(FlowBridge, "addNoteComment", async (id, content, replyToId) => {
    commentArgs = { id, content, replyToId };
    return {};
  });

  const textArea = fixture.page.find("textarea")[0];
  textArea.value = "Ideia sintética de teste";
  const saveBtn = fixture.page.find("button").find(b => b.textContent === "Salvar ideia em texto");
  saveBtn.fire("click");
  await flush();
  assert.deepEqual(createdArgs, { title: null, text: "Ideia sintética de teste" });

  const commentInput = fixture.page.find("input").find((i) => {
    const ph = i.getAttribute("placeholder") ?? "";
    return ph.includes("@persona") || ph.includes("observação");
  });
  commentInput.value = "@clara veja isso";
  const sendBtn = fixture.page.find("button").find(b => b.textContent === "Enviar");
  sendBtn.fire("click");
  await flush();
  assert.deepEqual(commentArgs, { id: "synthetic", content: "@clara veja isso", replyToId: null });
});

test("resolvePersonaAvatar converte asset://characters e bloqueia esquemas inseguros", () => {
  assert.equal(resolvePersonaAvatar("asset://characters/socio_tecnico.png"), "/characters/socio_tecnico.png");
  assert.equal(resolvePersonaAvatar("  /characters/curador.png  "), "/characters/curador.png");
  assert.equal(resolvePersonaAvatar("https://example.com/a.png"), "https://example.com/a.png");
  assert.equal(resolvePersonaAvatar("http://127.0.0.1:8080/a.png"), "http://127.0.0.1:8080/a.png");
  assert.equal(resolvePersonaAvatar("data:image/png;base64,AAAA"), "data:image/png;base64,AAAA");
  assert.equal(resolvePersonaAvatar("asset://characters/"), null);
  assert.equal(resolvePersonaAvatar("/characters/"), null);
  assert.equal(resolvePersonaAvatar("javascript:alert(1)"), null);
  assert.equal(resolvePersonaAvatar("data:text/html,<script>"), null);
  assert.equal(resolvePersonaAvatar("file:///etc/passwd"), null);
  assert.equal(resolvePersonaAvatar(""), null);
  assert.equal(resolvePersonaAvatar(null), null);
});

test("formatImportedPersonaFiles injeta id do arquivo em YAMLs sem id e une múltiplos com ---", () => {
  const joined = formatImportedPersonaFiles([
    { name: "socio_tecnico.yaml", content: "name: Sócio Técnico\nrole: Arquiteto" },
    { name: "curador.yml", content: "---\nid: curador_custom\nname: Curador" },
    { name: "advogado_diabo.yaml", content: "---\nname: Advogado" },
  ]);
  assert.equal(
    joined,
    "id: socio_tecnico\nname: Sócio Técnico\nrole: Arquiteto\n---\nid: curador_custom\nname: Curador\n---\nid: advogado_diabo\nname: Advogado",
  );
  const singleJson = formatImportedPersonaFiles([{ name: "p.json", content: '  [{"id":"a","name":"A"}]  ' }]);
  assert.equal(singleJson, '[{"id":"a","name":"A"}]');
});

test("Avatares com fallback onerror, exibição de firstMessage/mesExample e fluxos ADB/Restaurar padrões", async t => {
  t.mock.method(globalThis, "setInterval", () => 1);
  t.mock.method(globalThis, "clearInterval", () => {});
  const persona = {
    id: "socio_tecnico",
    name: "Sócio Técnico",
    role: "Arquiteto",
    avatar: "asset://characters/socio_tecnico.png",
    identity: "Direto e técnico",
    userProfile: "Dev",
    rules: "Aponte riscos",
    persona: "",
    firstMessage: "Qual gargalo vamos destravar hoje?",
    mesExample: "<START>\n{{user}}: E o deploy?\n{{char}}: Valide rollback antes.",
    tags: ["arquitetura"],
    interestFilter: "sistema",
    isPrimaryCompanion: true,
    enabled: true,
    color: "#3B9EFF",
  };
  const note = {
    id: "n1",
    title: "Nota sintética",
    titleEdited: false,
    start: Date.now() - 5000,
    status: "ready",
    speakers: [],
    lines: [{ id: "l1", speaker: "me", start: 0, end: 1, text: "Testar avatares" }],
    summary: "",
    keyPoints: [],
    todos: [],
    comments: [
      { id: "c1", characterId: "socio_tecnico", characterName: "Sócio Técnico", authorKind: "persona", content: "Veja os logs.", createdAt: 1 },
    ],
  };

  t.mock.method(internals, "invoke", async (cmd, args) => {
    if (cmd === "notes_state") return { notes: [note], recording: null, working: [] };
    if (cmd === "personas_state") return { personas: [persona], settings: { enableCharacterComment: true, maxCommentCharacters: 2 } };
    return originalInvoke(cmd, args);
  });

  const page = root();
  const ctx = { hero: () => root(), status: { models: { parakeet: { state: "ready" } } } };
  await notesPage(page, ctx);
  t.after(() => ctx.onLeave());

  // Comentário exibe <img> resolvido para /characters/... e faz fallback para inicial no onerror.
  const commentImg = page.find("img").find((img) => img.getAttribute("src") === "/characters/socio_tecnico.png");
  assert.ok(commentImg, "comentário deve renderizar <img> com caminho resolvido");
  assert.equal(commentImg.getAttribute("alt"), "Sócio Técnico");
  commentImg.fire("error");
  assert.equal(page.find("img").length, 0);
  assert.ok(page.find(".av").some((el) => el.textContent === "S"), "fallback deve exibir inicial colorida");

  // Abre o gerenciador de personas.
  page.find("button").find((b) => b.textContent.startsWith("Personas (")).fire("click");
  const cardImg = page.find("img").find((img) => img.getAttribute("src") === "/characters/socio_tecnico.png");
  assert.ok(cardImg, "card da persona deve renderizar avatar <img>");
  assert.match(page.textContent, /Qual gargalo vamos destravar hoje\?/);
  assert.match(page.textContent, /Valide rollback antes\./);
  cardImg.fire("error");
  assert.equal(page.find("img").length, 0);
  assert.ok(page.find(".av").some((el) => el.textContent === "S"), "fallback do card deve exibir inicial colorida");

  // Seleção de múltiplos arquivos .yaml no input preenche textarea concatenado.
  const fileInput = page.find("input").find((i) => i.getAttribute("type") === "file");
  assert.equal(fileInput.getAttribute("accept"), ".json,.yaml,.yml,application/json,text/yaml");
  assert.notEqual(fileInput.getAttribute("multiple"), null);
  fileInput.files = [
    { name: "curador.yaml", text: async () => "name: Curador" },
    { name: "tradutor.yml", text: async () => "id: tradutor_id\nname: Tradutor" },
  ];
  fileInput.fire("change");
  await flush();
  const importArea = page.find("textarea").find((ta) => ta.getAttribute("placeholder")?.includes(".yaml"));
  assert.equal(importArea.value, "id: curador\nname: Curador\n---\nid: tradutor_id\nname: Tradutor");

  // Fluxo ADB: estado de loading + sucesso; depois erro e sucesso em Restaurar 15 padrões.
  let resolveAdb;
  t.mock.method(FlowBridge, "syncPersonasAdb", () => new Promise((resolve) => { resolveAdb = resolve; }));
  const adbBtn = page.find("button").find((b) => b.textContent === "Sincronizar do celular (ADB)");
  adbBtn.fire("click");
  const syncingBtn = page.find("button").find((b) => b.textContent === "Sincronizando…");
  assert.ok(syncingBtn);
  assert.equal(syncingBtn.disabled, true);
  assert.equal(page.find("button").find((b) => b.textContent === "Restaurar 15 padrões").disabled, true);
  resolveAdb({ personas: [persona], settings: { enableCharacterComment: true, maxCommentCharacters: 2 } });
  await flush();
  assert.match(page.textContent, /Personas sincronizadas do celular \(1 no total\)\./);

  let failReset = true;
  t.mock.method(FlowBridge, "resetDefaultPersonas", async () => {
    if (failReset) throw new Error("Falha sintética ao restaurar padrões");
    return { personas: [persona], settings: { enableCharacterComment: true, maxCommentCharacters: 2 } };
  });
  page.find("button").find((b) => b.textContent === "Restaurar 15 padrões").fire("click");
  await flush();
  assert.match(page.textContent, /Falha sintética ao restaurar padrões/);

  failReset = false;
  page.find("button").find((b) => b.textContent === "Restaurar 15 padrões").fire("click");
  await flush();
  assert.match(page.textContent, /1 personas padrão restauradas\./);
});

test("Anotador valida nota vazia, adapta contraste de avatar, marca persona desativada e responde comentário sem side-tab", async t => {
  t.mock.method(globalThis, "setInterval", () => 1);
  t.mock.method(globalThis, "clearInterval", () => {});
  const personas = [
    {
      id: "clara",
      name: "Clara",
      role: "Estrategista",
      avatar: "",
      identity: "Direta",
      userProfile: "",
      rules: "",
      persona: "",
      firstMessage: "",
      mesExample: "",
      tags: ["foco"],
      interestFilter: "",
      isPrimaryCompanion: true,
      enabled: true,
      color: "#E99A3D",
    },
    {
      id: "noite",
      name: "Noite",
      role: "Revisora",
      avatar: "",
      identity: "Calma",
      userProfile: "",
      rules: "",
      persona: "",
      firstMessage: "",
      mesExample: "",
      tags: [],
      interestFilter: "",
      isPrimaryCompanion: false,
      enabled: false,
      color: "#312E81",
    },
  ];
  const note = {
    id: "n-reply",
    title: "Ideia editorial",
    titleEdited: true,
    start: Date.now() - 1000,
    status: "ready",
    speakers: [],
    lines: [{ id: "l1", speaker: "me", start: 0, end: 2, text: "Testar fluxo de resposta." }],
    summary: "Resumo sintético.",
    keyPoints: [],
    todos: [],
    comments: [
      { id: "c1", characterId: "clara", characterName: "Clara", authorKind: "persona", content: "Revise o escopo.", createdAt: 1 },
    ],
  };

  t.mock.method(internals, "invoke", async (cmd, args) => {
    if (cmd === "notes_state") return { notes: [note], recording: null, working: [] };
    if (cmd === "personas_state") return { personas, settings: { enableCharacterComment: true, maxCommentCharacters: 2 } };
    return originalInvoke(cmd, args);
  });

  const page = root();
  const ctx = { hero: () => root(), status: { models: { parakeet: { state: "ready" } } } };
  await notesPage(page, ctx);
  t.after(() => ctx.onLeave());

  // Salvar ideia vazia exibe aviso sem disparar IPC.
  page.find("button").find((b) => b.textContent === "Salvar ideia em texto").fire("click");
  assert.match(page.textContent, /Escreva uma ideia antes de salvar\./);

  // Avatar claro usa tinta escura (#0b0c0e) e comentário não usa border-inline-start inline.
  const claraAv = page.find(".av").find((el) => el.textContent === "C");
  assert.match(claraAv.getAttribute("style") ?? "", /color:#0b0c0e/);
  const commentNode = page.find(".notes-comment")[0];
  assert.doesNotMatch(commentNode.getAttribute("style") ?? "", /border-inline-start/);

  // Fluxo de responder e cancelar resposta.
  page.find("button").find((b) => b.textContent === "Responder").fire("click");
  assert.match(page.textContent, /Respondendo a Clara/);
  page.find("button").find((b) => b.textContent === "Cancelar resposta").fire("click");
  assert.doesNotMatch(page.textContent, /Respondendo a Clara/);

  // Abre Personas: exibe badge "Desativada" e avatar escuro (#312E81) usa tinta clara (#F3EEE6).
  page.find("button").find((b) => b.textContent.startsWith("Personas (")).fire("click");
  assert.match(page.textContent, /Desativada/);
  const noiteAv = page.find(".av").find((el) => el.textContent === "N");
  assert.match(noiteAv.getAttribute("style") ?? "", /color:#F3EEE6/);

  // Volta às notas pelo cabeçalho do gerenciador.
  page.find("button").find((b) => b.textContent === "← Voltar à nota").fire("click");
  assert.match(page.textContent, /Comentários das personas \(1\)/);
});

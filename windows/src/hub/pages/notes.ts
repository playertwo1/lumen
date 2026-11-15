// Anotador: captura de ideias e pensamentos por voz (microfone) ou texto rápido,
// com resumo pelo provedor ativo e comentários das personas configuradas.

import { h, clear } from "../../views/dom";
import { t, LUMEN_WINDOWS } from "../../i18n/i18n";
import { onEvent } from "../../core/bridge";
import {
  FlowBridge,
  clock,
  noteMatches,
  speakerColor,
  type CommentSettings,
  type Note,
  type NotesState,
  type PersonasState,
} from "../../core/flow";
import type { HubContext } from "../shell";
import { emptyState, glowCard } from "../hero";
import { button, iconButton, segmented } from "../ui";
import { modelsControl } from "./flowsettings";

const lang = () => document.documentElement.lang || undefined;

const SYNTHETIC_PERSONA_SAMPLE = JSON.stringify(
  [
    {
      id: "clara",
      name: "Clara",
      role: "Estrategista",
      identity: "Direta, prática e gentil. Vai direto ao ponto sem enrolação.",
      userProfile: "Pessoa organizando projetos, prioridades e ideias do dia.",
      rules: "Destaque o próximo passo concreto ou risco ignorado em até 2 frases.",
      tags: ["projeto", "produto", "foco"],
      interestFilter: "foco, prioridades, arquitetura",
      isPrimaryCompanion: true,
    },
  ],
  null,
  2,
);

function defaultPersonasState(): PersonasState {
  return {
    personas: [],
    settings: { enableCharacterComment: true, maxCommentCharacters: 15 },
  };
}

/** Converts `asset://characters/<file>` to `/characters/<file>` and keeps safe avatar URLs. */
export function resolvePersonaAvatar(avatar?: string | null): string | null {
  const value = avatar?.trim();
  if (!value) return null;
  if (value.startsWith("asset://characters/")) {
    const file = value.slice("asset://characters/".length);
    return file ? `/characters/${file}` : null;
  }
  if (
    (value.startsWith("/characters/") && value.length > "/characters/".length) ||
    value.startsWith("https://") ||
    value.startsWith("http://") ||
    value.startsWith("data:image/")
  ) {
    return value;
  }
  return null;
}

/** Normalizes selected .json/.yaml files for `personas_import`, injecting `id: <basename>` into YAMLs without `id:`. */
export function formatImportedPersonaFiles(files: { name: string; content: string }[]): string {
  return files
    .map(({ name, content }) => {
      const trimmed = content.replace(/^\uFEFF/, "").trim();
      if (!trimmed || !/\.(?:ya?ml)$/i.test(name)) return trimmed;
      const body = trimmed.replace(/^---\r?\n/, "");
      if (/^id\s*:/m.test(body)) return body;
      const baseId = (name.split(/[\\/]/).pop() ?? name).replace(/\.(?:ya?ml)$/i, "").trim();
      return baseId ? `id: ${baseId}\n${body}` : body;
    })
    .filter(Boolean)
    .join("\n---\n");
}

function personaAvatar(name: string, color: string, avatar?: string | null, size = 28): HTMLElement {
  const initial = (name.trim().slice(0, 1) || "?").toUpperCase();
  const circleStyle = `background:${color};width:${size}px;height:${size}px;border-radius:999px;display:inline-flex;align-items:center;justify-content:center;font:700 ${Math.max(11, Math.round(size * 0.45))}px var(--font);color:#fff;flex-shrink:0;overflow:hidden`;
  const src = resolvePersonaAvatar(avatar);
  if (!src) {
    return h("span", { class: "av", style: circleStyle, text: initial });
  }
  const wrap = h("span", { class: "av-wrap", style: circleStyle });
  const img = h("img", {
    src,
    alt: name,
    style: "width:100%;height:100%;object-fit:cover;display:block;border-radius:999px",
    onerror: () => {
      wrap.replaceChildren(h("span", { class: "av", style: circleStyle, text: initial }));
    },
  });
  wrap.append(img);
  return wrap;
}

export async function notesPage(page: HTMLElement, ctx: HubContext) {
  let state: NotesState = (await FlowBridge.notes()) ?? { notes: [], recording: null, working: [] };
  let personasState: PersonasState = (await FlowBridge.personasState()) ?? defaultPersonasState();
  let selected: string | null = state.recording ?? state.notes[0]?.id ?? null;
  let tab: "summary" | "transcript" = "summary";
  let showPersonas = false;
  let startError = "";
  let quickError = "";
  let commentError = "";
  let personaFeedback = "";
  let personaError = "";
  let personaBusy: "" | "import" | "adb" | "reset" = "";
  let personaDraft = "";
  let replyToId: string | null = null;
  let left = false, exporting = false, savingText = false;
  let exportResult: { id: string; path?: string; error?: string } | null = null;

  const heroTitle = LUMEN_WINDOWS ? "Anotador" : t("Notetaker");
  const heroSubtitle = "Dite ideias no microfone ou salve pensamentos em texto, com resumo e comentários das suas personas.";
  page.append(ctx.hero("notes", heroTitle, heroSubtitle, [], true));

  if (ctx.status.models.parakeet.state !== "ready") {
    page.append(glowCard(
      "#3B9EFF",
      h("div", { style: "font:700 15px var(--font)", text: "Modelo de voz opcional para ditado" }),
      h("div", {
        class: "hub-note",
        style: "color:#b7bbc2",
        text: LUMEN_WINDOWS
          ? `Notas em texto já funcionam agora. Para ditar ideias por voz offline, baixe o Parakeet (${ctx.status.models.downloadMb ?? 0} MB).`
          : t("Coucou writes your calls on this computer with Parakeet, and tells the voices apart with a second small model. Both are downloaded once (about {0} MB), then everything works offline.", { 0: ctx.status.models.downloadMb }),
      }),
      modelsControl(ctx, () => ctx.navigate("notes")),
    ));
  }

  let query = "";
  const search = h("input", { class: "hub-input", placeholder: t("Search"), type: "search" }) as HTMLInputElement;
  search.addEventListener("input", () => {
    query = search.value;
    drawList(true);
  });

  const quickTitle = h("input", {
    class: "hub-input",
    placeholder: "Título opcional",
    style: "width:100%",
  }) as HTMLInputElement;
  const quickText = h("textarea", {
    class: "hub-input",
    rows: "3",
    placeholder: "Escreva uma ideia ou pensamento rápido… (Ctrl+Enter)",
    style: "width:100%;resize:vertical;min-height:66px;font-family:var(--font)",
  }) as HTMLTextAreaElement;

  const listEl = h("div", { class: "hub-notes-list" });
  const detail = h("div", { class: "hub-note-detail" });
  const notesContainer = h("div", { class: "hub-notes" }, listEl, detail);
  notesContainer.dataset.anchor = "flow.notes";
  page.append(notesContainer);

  const reload = async () => {
    state = (await FlowBridge.notes()) ?? state;
    personasState = (await FlowBridge.personasState()) ?? personasState;
    if (selected && !state.notes.some((n) => n.id === selected)) selected = state.notes[0]?.id ?? null;
    drawList();
    drawDetail();
  };

  async function saveQuickIdea() {
    if (savingText) return;
    const text = (quickText.value ?? "").trim();
    if (!text) {
      quickError = "Escreva uma ideia antes de salvar.";
      drawList();
      return;
    }
    savingText = true;
    quickError = "";
    try {
      const created = await FlowBridge.createTextNote((quickTitle.value ?? "").trim() || null, text);
      quickTitle.value = "";
      quickText.value = "";
      if (created?.id) selected = created.id;
      showPersonas = false;
      await reload();
    } catch (err) {
      quickError = String(err).replace(/^Error:\s*/, "");
      drawList();
    } finally {
      savingText = false;
    }
  }

  quickText.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      void saveQuickIdea();
    }
  });

  function recordButton(): HTMLElement {
    if (state.recording) {
      const rec = state.notes.find((n) => n.id === state.recording);
      const b = button(`■  ${t("Stop")} · ${clock((Date.now() - (rec?.start ?? Date.now())) / 1000)}`, () => void FlowBridge.stopNote(), "danger");
      b.dataset.ticking = "1";
      return b;
    }
    const b = button("Ditar ideia", () => {
      startError = "";
      FlowBridge.startNote().then((id) => {
        selected = id;
        showPersonas = false;
        void reload();
      }, (err) => {
        startError = String(err).replace(/^Error:\s*/, "");
        drawList();
      });
    }, "primary");
    b.prepend(h("span", { class: "hub-rec", style: "background:#170c04;box-shadow:none;margin-inline-end:8px" }));
    return b;
  }

  function drawList(typing = false) {
    clear(listEl);
    const rb = recordButton();
    rb.style.width = "100%";
    listEl.append(rb);

    const activePersonasCount = personasState.personas.filter((p) => p.enabled).length;
    const personasBtn = button(
      showPersonas ? "← Voltar às notas" : `Personas (${activePersonasCount} ativa${activePersonasCount === 1 ? "" : "s"})`,
      () => {
        showPersonas = !showPersonas;
        drawList();
        drawDetail();
      },
      showPersonas ? "primary" : "",
      true,
    );
    personasBtn.style.width = "100%";
    listEl.append(personasBtn);

    const saveBtn = button(savingText ? "Salvando…" : "Salvar ideia em texto", () => void saveQuickIdea(), "", true);
    saveBtn.style.width = "100%";
    listEl.append(
      h("div", { class: "hub-card", style: "padding:10px;display:flex;flex-direction:column;gap:6px" },
        h("div", { class: "hub-group", style: "margin:0", text: "Nova nota rápida" }),
        quickTitle,
        quickText,
        quickError ? h("div", { class: "hub-warn", text: quickError }) : null,
        saveBtn,
      ),
    );

    if (state.notes.length > 0) listEl.append(search);
    if (typing) search.focus();
    if (startError) listEl.append(h("div", { class: "hub-warn", text: startError }));
    if (state.notes.length === 0) {
      listEl.append(h("div", { class: "hub-note", style: "padding:10px 4px", text: "Suas ideias e anotações aparecem aqui. Dite no microfone ou salve em texto." }));
    }

    let day = "";
    for (const n of state.notes.filter((x) => noteMatches(x, query))) {
      const d = new Date(n.start).toDateString();
      if (d !== day) {
        day = d;
        const isToday = d === new Date().toDateString();
        listEl.append(h("div", {
          class: "hub-group",
          style: "padding:10px 6px 2px",
          text: isToday ? t("Today") : new Date(n.start).toLocaleDateString(lang(), { day: "numeric", month: "long" }),
        }));
      }
      const commentCount = n.comments?.length ?? 0;
      const meta = n.status === "recording"
        ? t("Recording…")
        : state.working.includes(n.id) || n.status === "processing"
          ? t("writing…")
          : n.status === "failed"
            ? t("failed")
            : `${new Date(n.start).toLocaleTimeString(lang(), { hour: "2-digit", minute: "2-digit" })} · ${commentCount} coment.`;
      const item = h("button", {
        class: n.id === selected && !showPersonas ? "hub-note-item on" : "hub-note-item",
        onclick: () => {
          selected = n.id;
          showPersonas = false;
          replyToId = null;
          commentError = "";
          drawList();
          drawDetail();
        },
      }, h("span", { class: "t", text: n.title }), h("span", { class: "m", text: meta }));
      if (n.status === "recording") item.querySelector(".m")!.prepend(h("span", { class: "hub-rec", style: "margin-inline-end:6px" }));
      listEl.append(item);
    }

    listEl.append(
      h("div", { style: "flex:1" }),
      h("div", {
        class: "hub-note",
        style: "padding:10px;border-radius:10px;background:rgba(255,255,255,0.04)",
        text: "Tudo fica salvo localmente. Notas por voz usam apenas o microfone e seu provedor ativo.",
      }),
    );
  }

  function renderPersonasManager() {
    clear(detail);
    const settings: CommentSettings = personasState.settings;
    const enableBox = h("input", { type: "checkbox" }) as HTMLInputElement;
    enableBox.checked = settings.enableCharacterComment;
    const maxInput = h("input", {
      class: "hub-input",
      type: "number",
      min: "1",
      max: "15",
      value: String(settings.maxCommentCharacters),
      style: "width:76px",
    }) as HTMLInputElement;

    const updateSettings = async () => {
      personaError = "";
      personaFeedback = "";
      const maxChars = Math.min(15, Math.max(1, Number(maxInput.value) || 15));
      try {
        personasState = await FlowBridge.setCommentSettings(enableBox.checked, maxChars);
        personaFeedback = "Regras de comentário atualizadas.";
        drawList();
        renderPersonasManager();
      } catch (err) {
        personaError = String(err).replace(/^Error:\s*/, "");
        renderPersonasManager();
      }
    };
    enableBox.addEventListener("change", () => void updateSettings());
    maxInput.addEventListener("change", () => void updateSettings());

    const jsonBox = h("textarea", {
      class: "hub-input",
      rows: "7",
      placeholder: "Cole JSON ou .yaml da Segunda Mente (junte vários docs com ---) ou 3 partes QwenPaw (identity, userProfile, rules)…",
      style: "width:100%;resize:vertical;font:12.5px/1.45 var(--mono, monospace)",
    }) as HTMLTextAreaElement;
    jsonBox.value = personaDraft;
    jsonBox.addEventListener("input", () => {
      personaDraft = jsonBox.value;
    });

    const fileInput = h("input", {
      type: "file",
      accept: ".json,.yaml,.yml,application/json,text/yaml",
      multiple: true,
      style: "display:none",
    }) as HTMLInputElement;
    fileInput.addEventListener("change", () => {
      const files = Array.from(fileInput.files ?? []);
      if (files.length === 0) return;
      void Promise.all(files.map(async (file) => ({ name: file.name, content: await file.text() })))
        .then((loaded) => {
          personaDraft = formatImportedPersonaFiles(loaded);
          jsonBox.value = personaDraft;
          fileInput.value = "";
        })
        .catch((err) => {
          personaError = String(err).replace(/^Error:\s*/, "");
          renderPersonasManager();
        });
    });

    async function runPersonaAction(
      kind: "import" | "adb" | "reset",
      task: () => Promise<PersonasState>,
      onSuccess: (next: PersonasState) => string,
    ) {
      if (personaBusy) return;
      personaBusy = kind;
      personaError = "";
      personaFeedback = "";
      renderPersonasManager();
      try {
        personasState = await task();
        personaFeedback = onSuccess(personasState);
        drawList();
      } catch (err) {
        personaError = String(err).replace(/^Error:\s*/, "");
      } finally {
        personaBusy = "";
        if (!left) renderPersonasManager();
      }
    }

    const importAction = () => {
      const raw = jsonBox.value;
      personaDraft = raw;
      return runPersonaAction(
        "import",
        () => FlowBridge.importPersonas(raw),
        (next) => {
          personaDraft = "";
          return `Personas salvas (${next.personas.length} no total).`;
        },
      );
    };
    const syncAdbAction = () =>
      runPersonaAction(
        "adb",
        () => FlowBridge.syncPersonasAdb(),
        (next) => `Personas sincronizadas do celular (${next.personas.length} no total).`,
      );
    const resetDefaultsAction = () =>
      runPersonaAction(
        "reset",
        () => FlowBridge.resetDefaultPersonas(),
        (next) => `${next.personas.length} personas padrão restauradas.`,
      );

    const busy = personaBusy !== "";
    const importBtn = button(personaBusy === "import" ? "Importando…" : "Importar JSON/YAML", () => void importAction(), "primary", true);
    const chooseBtn = button("Escolher arquivos (.json/.yaml)", () => fileInput.click(), "", true);
    const syncAdbBtn = button(personaBusy === "adb" ? "Sincronizando…" : "Sincronizar do celular (ADB)", () => void syncAdbAction(), "", true);
    const resetBtn = button(personaBusy === "reset" ? "Restaurando…" : "Restaurar 15 padrões", () => void resetDefaultsAction(), "", true);
    const sampleBtn = button("Preencher exemplo sintético", () => {
      personaDraft = SYNTHETIC_PERSONA_SAMPLE;
      jsonBox.value = personaDraft;
    }, "", true);
    for (const b of [importBtn, chooseBtn, syncAdbBtn, resetBtn, sampleBtn]) b.disabled = busy;

    detail.append(
      h("div", { style: "display:flex;align-items:center;gap:10px;justify-content:space-between" },
        h("h2", { style: "margin:0", text: "Personas e regras de comentário" }),
        button("← Voltar à nota", () => {
          showPersonas = false;
          drawList();
          drawDetail();
        }, "", true),
      ),
      h("div", { class: "hub-card", style: "display:flex;flex-direction:column;gap:10px" },
        h("label", { class: "hub-row", style: "gap:8px;cursor:pointer" },
          enableBox,
          h("span", { text: "Permitir comentários automáticos após criar ou ditar uma nota" }),
        ),
        h("div", { class: "hub-row", style: "gap:10px;align-items:center" },
          h("span", { class: "hub-note", text: "Máximo de personas por nota (até 15 no chat em grupo, se pertinente):" }),
          maxInput,
          h("span", { class: "hub-note", text: "Dica: @nome sempre responde mesmo se o automático estiver desligado." }),
        ),
      ),
      h("div", { class: "hub-card", style: "display:flex;flex-direction:column;gap:10px" },
        h("div", { style: "font:600 14px var(--font)", text: "Importar ou sincronizar personas (JSON ou .yaml da Segunda Mente)" }),
        fileInput,
        jsonBox,
        h("div", { class: "hub-row", style: "gap:8px;flex-wrap:wrap" },
          importBtn,
          chooseBtn,
          syncAdbBtn,
          resetBtn,
          sampleBtn,
        ),
        personaFeedback ? h("div", { class: "hub-note", style: "color:#34D399", text: personaFeedback }) : null,
        personaError ? h("div", { class: "hub-warn", text: personaError }) : null,
      ),
    );

    if (personasState.personas.length === 0) {
      detail.append(h("div", { class: "hub-note", text: "Nenhuma persona importada ainda." }));
      return;
    }

    for (const p of personasState.personas) {
      detail.append(
        h("div", { class: "hub-card", style: "display:flex;flex-direction:column;gap:8px" },
          h("div", { class: "hub-row", style: "justify-content:space-between;align-items:center;gap:8px;flex-wrap:wrap" },
            h("div", { class: "hub-row", style: "gap:10px;align-items:center" },
              personaAvatar(p.name, p.color, p.avatar, 36),
              h("div", { style: "display:flex;flex-direction:column;gap:2px" },
                h("div", { class: "hub-row", style: "gap:6px;align-items:center;flex-wrap:wrap" },
                  h("strong", { text: `${p.name} (@${p.id})` }),
                  p.isPrimaryCompanion ? h("span", { class: "hub-note", style: "color:#34D399", text: "Principal" }) : null,
                ),
                p.role ? h("div", { class: "hub-note", text: p.role }) : null,
              ),
            ),
            h("div", { class: "hub-row", style: "gap:6px" },
              button(p.enabled ? "Desativar" : "Ativar", async () => {
                personaError = "";
                try {
                  personasState = await FlowBridge.togglePersona(p.id, !p.enabled);
                  drawList();
                } catch (err) {
                  personaError = String(err).replace(/^Error:\s*/, "");
                }
                renderPersonasManager();
              }, "", true),
              !p.isPrimaryCompanion ? button("Definir principal", async () => {
                personaError = "";
                try {
                  personasState = await FlowBridge.setPrimaryPersona(p.id);
                } catch (err) {
                  personaError = String(err).replace(/^Error:\s*/, "");
                }
                renderPersonasManager();
              }, "", true) : null,
              button(t("Delete"), async () => {
                personaError = "";
                try {
                  personasState = await FlowBridge.deletePersona(p.id);
                  drawList();
                } catch (err) {
                  personaError = String(err).replace(/^Error:\s*/, "");
                }
                renderPersonasManager();
              }, "danger", true),
            ),
          ),
          p.tags.length ? h("div", { class: "hub-note", text: `Tags: ${p.tags.join(", ")}` }) : null,
          p.interestFilter ? h("div", { class: "hub-note", text: `Foco: ${p.interestFilter}` }) : null,
          p.identity ? h("div", { class: "hub-note", text: `Identidade: ${p.identity}` }) : null,
          p.rules ? h("div", { class: "hub-note", text: `Regras: ${p.rules}` }) : null,
          p.firstMessage?.trim()
            ? h("div", {
                class: "hub-note",
                style: "padding:6px 10px;border-inline-start:2px solid rgba(255,255,255,0.16);white-space:pre-wrap",
                text: `Primeira fala: ${p.firstMessage.trim()}`,
              })
            : null,
          p.mesExample?.trim()
            ? h("details", { class: "hub-note" },
                h("summary", { style: "cursor:pointer;user-select:none", text: "Exemplo de diálogo" }),
                h("div", {
                  style: "margin-top:6px;padding:6px 10px;background:rgba(255,255,255,0.03);border-radius:8px;white-space:pre-wrap",
                  text: p.mesExample.trim(),
                }),
              )
            : null,
        ),
      );
    }
  }

  function renderComments(note: Note): HTMLElement {
    const section = h("div", { class: "hub-card", style: "display:flex;flex-direction:column;gap:12px;align-self:stretch" });
    const comments = note.comments ?? [];
    const busy = state.working.includes(note.id);
    const activeCount = personasState.personas.filter((p) => p.enabled).length;

    const header = h("div", { class: "hub-row", style: "justify-content:space-between;align-items:center;gap:8px;flex-wrap:wrap" },
      h("div", { style: "font:700 14px var(--font)", text: `Comentários das personas (${comments.length})` }),
      h("div", { class: "hub-row", style: "gap:8px" },
        button(busy ? "Gerando…" : "Gerar comentários", async () => {
          commentError = "";
          try {
            await FlowBridge.generateNoteComments(note.id);
            await reload();
          } catch (err) {
            commentError = String(err).replace(/^Error:\s*/, "");
            drawDetail();
          }
        }, "", true),
        button("Gerenciar personas", () => {
          showPersonas = true;
          drawList();
          drawDetail();
        }, "", true),
      ),
    );
    section.append(header);

    if (commentError) {
      section.append(h("div", { class: "hub-warn", text: commentError }));
    }

    if (comments.length === 0) {
      const hint = activeCount === 0
        ? "Importe e ative uma persona para receber comentários automáticos nas suas notas."
        : "Nenhum comentário ainda. Clique em Gerar comentários ou mencione @persona abaixo.";
      section.append(h("div", { class: "hub-note", text: hint }));
    } else {
      const byId = new Map(comments.map((c) => [c.id, c]));
      for (const c of comments) {
        const persona = personasState.personas.find((p) => p.id === c.characterId);
        const color = c.authorKind === "user" ? "#3B9EFF" : (persona?.color ?? "#6C5CE7");
        const parent = c.replyToId ? byId.get(c.replyToId) : undefined;
        section.append(
          h("div", {
            style: `border-inline-start:3px solid ${color};padding:6px 10px;background:rgba(255,255,255,0.03);border-radius:8px;display:flex;flex-direction:column;gap:4px`,
          },
            h("div", { class: "hub-row", style: "justify-content:space-between;align-items:center;gap:8px" },
              h("div", { class: "hub-row", style: "gap:6px;align-items:center" },
                personaAvatar(c.characterName, color, c.authorKind === "user" ? null : persona?.avatar, 24),
                h("strong", { style: `color:${color};font-size:13px`, text: c.characterName }),
                parent ? h("span", { class: "hub-note", text: `↳ em resposta a ${parent.characterName}` }) : null,
              ),
              c.authorKind !== "user" ? button("Responder", () => {
                replyToId = c.id;
                drawDetail();
              }, "", true) : null,
            ),
            h("div", { style: "font-size:13.5px;line-height:1.45;color:var(--hub-body)", text: c.content }),
          ),
        );
      }
    }

    const replyTarget = replyToId ? comments.find((c) => c.id === replyToId) : undefined;
    if (replyTarget) {
      section.append(
        h("div", { class: "hub-row", style: "gap:8px;align-items:center" },
          h("span", { class: "hub-note", text: `Respondendo a ${replyTarget.characterName}` }),
          button("Cancelar resposta", () => {
            replyToId = null;
            drawDetail();
          }, "", true),
        ),
      );
    }

    const replyInput = h("input", {
      class: "hub-input",
      placeholder: activeCount > 0 ? "Comentar ou mencionar @persona…" : "Adicionar observação nesta nota…",
      style: "flex:1",
    }) as HTMLInputElement;
    const sendReply = async () => {
      const content = (replyInput.value ?? "").trim();
      if (!content) return;
      commentError = "";
      try {
        await FlowBridge.addNoteComment(note.id, content, replyToId);
        replyInput.value = "";
        replyToId = null;
        await reload();
      } catch (err) {
        commentError = String(err).replace(/^Error:\s*/, "");
        drawDetail();
      }
    };
    replyInput.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        void sendReply();
      }
    });
    section.append(
      h("div", { class: "hub-row", style: "gap:8px;align-items:center" },
        replyInput,
        button("Enviar", () => void sendReply(), "primary", true),
      ),
    );
    return section;
  }

  function drawDetail() {
    if (showPersonas) {
      renderPersonasManager();
      return;
    }
    clear(detail);
    const note = state.notes.find((n) => n.id === selected);
    if (!note) {
      detail.append(emptyState("Suas ideias e pensamentos aparecerão aqui. Dite pelo microfone ou registre em texto.", "sleeping", "#3B9EFF"));
      return;
    }

    const title = h("h2", { text: note.title, title: t("Edit"), style: "cursor:text" });
    title.addEventListener("click", () => {
      const input = h("input", { class: "hub-input", style: "font:700 20px var(--font);width:100%" }) as HTMLInputElement;
      input.value = note.title;
      const done = () => void FlowBridge.renameNote(note.id, input.value);
      input.addEventListener("keydown", (e) => {
        if (e.key === "Enter") done();
        if (e.key === "Escape") drawDetail();
      });
      input.addEventListener("blur", done);
      title.replaceWith(input);
      input.focus();
      input.select();
    });
    const when = new Date(note.start).toLocaleString(lang(), { weekday: "long", day: "numeric", month: "long", hour: "2-digit", minute: "2-digit" });
    const copied = h("span", { class: "hub-note" });
    const tools = h("div", { class: "hub-row", style: "gap:8px" });
    const confirm = h("div", {});
    if (note.status === "ready") {
      const noteId = note.id;
      tools.append(button(t("Copy as Markdown"), async () => {
        const md = await FlowBridge.markdown(note.id);
        if (md && (await FlowBridge.copy(md))) {
          copied.textContent = t("Copied");
          setTimeout(() => (copied.textContent = ""), 1500);
        }
      }, "", true));
      const exportControl = button(t("Export as Markdown…"), () => void exportNote(), "", true);
      exportControl.disabled = exporting;
      tools.append(exportControl);
      async function exportNote() {
        if (exporting || left) return;
        exporting = true;
        exportResult = null;
        exportControl.disabled = true;
        try {
          const path = await FlowBridge.exportNote(noteId);
          if (path) exportResult = { id: noteId, path };
        } catch (error) {
          exportResult = { id: noteId, error: String(error).replace(/^Error:\s*/, "") };
        } finally {
          exporting = false;
          if (!left) drawDetail();
        }
      }
    }
    if (note.status !== "recording") {
      tools.append(iconButton("trash", t("Delete"), () => {
        clear(confirm);
        confirm.append(h("div", { class: "hub-warn", style: "display:flex;align-items:center;gap:10px" },
          h("span", { style: "flex:1", text: "Excluir esta anotação e seus comentários?" }),
          button(t("Delete"), () => void FlowBridge.deleteNote(note.id), "danger", true),
          button(t("Cancel"), () => clear(confirm), "", true),
        ));
      }));
    }
    tools.append(copied);
    detail.append(
      h("div", { style: "display:flex;flex-direction:column;gap:4px;min-width:0" }, title, h("div", { class: "hub-note", text: note.app ? `${when} · ${note.app}` : when })),
      tools,
      confirm,
    );
    if (exportResult?.id === note.id) {
      detail.append(h("div", {
        class: exportResult.error ? "hub-warn" : "hub-note",
        role: exportResult.error ? "alert" : "status",
        style: "overflow-wrap:anywhere",
        text: exportResult.error ?? t("Saved as {name}", { name: exportResult.path ?? "" }),
      }));
    }

    if (note.speakers.length) {
      const people = h("div", { class: "hub-speakers" });
      for (const s of note.speakers) {
        const chip = h("button", { class: "hub-speaker", title: t("Who is it?") },
          h("span", { class: "av", style: `background:${speakerColor(s.color)}`, text: s.name.slice(0, 1).toUpperCase() }),
          h("span", { text: s.name }),
        );
        chip.addEventListener("click", () => {
          const input = h("input", { class: "hub-input", placeholder: t("Name"), style: "width:150px" }) as HTMLInputElement;
          input.value = s.name;
          const done = () => void FlowBridge.renameSpeaker(note.id, s.id, input.value);
          input.addEventListener("keydown", (e) => e.key === "Enter" && done());
          input.addEventListener("blur", done);
          chip.replaceWith(input);
          input.focus();
          input.select();
        });
        people.append(chip);
      }
      detail.append(people);
    }

    if (note.status === "recording") {
      detail.append(h("div", { class: "hub-row" }, h("span", { class: "hub-rec" }), h("b", { text: t("Live") })));
      detail.append(transcript(note, t("The words show up here as people speak.")));
      return;
    }
    if (note.status === "processing" || (state.working.includes(note.id) && note.lines.length === 0)) {
      detail.append(emptyState("Transcrevendo e organizando sua ideia…", "working", "#3B9EFF"));
      return;
    }
    if (note.status === "failed") {
      detail.append(h("div", { class: "hub-warn", text: `Não foi possível processar esta gravação. ${note.error ?? ""}` }));
      detail.append(button(t("Retry"), () => void FlowBridge.retryNote(note.id)));
      return;
    }
    if (note.lines.length === 0) {
      detail.append(emptyState("Nenhuma fala foi captada nesta gravação.", "idle", "#3B9EFF"));
      return;
    }

    const tabs = segmented<"summary" | "transcript">([["summary", "Resumo & Personas"], ["transcript", "Texto / Transcrição"]], tab, (next) => {
      tab = next;
      drawDetail();
    });
    detail.append(h("div", {}, tabs));
    if (tab === "summary") {
      detail.append(summary(note));
      detail.append(renderComments(note));
    } else {
      detail.append(transcript(note, ""));
      detail.append(renderComments(note));
    }
  }

  function summary(note: Note): HTMLElement {
    const el = h("div", { style: "display:flex;flex-direction:column;gap:14px;align-items:flex-start" });
    const busy = state.working.includes(note.id);
    if (!note.summary && note.keyPoints.length === 0 && note.todos.length === 0) {
      const rawText = note.lines.map((l) => l.text).join("\n");
      if (rawText) {
        el.append(h("div", { class: "hub-card", style: "font-size:14px;line-height:1.55;color:var(--hub-body);align-self:stretch;white-space:pre-wrap", text: rawText }));
      } else {
        el.append(h("div", { class: "hub-note", text: busy ? "Gerando resumo…" : "Ainda sem resumo automático." }));
      }
      if (note.error) {
        el.append(h("div", { class: "hub-warn", role: "alert", text: note.error }));
      }
      if (!busy) el.append(button("Gerar resumo", () => void FlowBridge.summarize(note.id), "primary", true));
      return el;
    }
    if (note.summary) el.append(h("div", { class: "hub-card", style: "font-size:14px;line-height:1.55;color:var(--hub-body);align-self:stretch", text: note.summary }));
    if (note.keyPoints.length) {
      const ul = h("ul", { style: "margin:0;padding-inline-start:20px;display:flex;flex-direction:column;gap:6px;font-size:13.5px" });
      for (const p of note.keyPoints) ul.append(h("li", { text: p }));
      el.append(h("div", { class: "hub-label", text: "Pontos principais" }), ul);
    }
    if (note.todos.length) {
      el.append(h("div", { class: "hub-label", text: "Ações / To-do" }));
      for (const todo of note.todos) {
        const box = h("input", { type: "checkbox" }) as HTMLInputElement;
        box.checked = todo.done;
        box.addEventListener("change", () => void FlowBridge.toggleTodo(note.id, todo.id));
        const extra = [todo.owner, todo.due].filter(Boolean).join(" · ");
        el.append(h("label", { class: "hub-todo" }, box, h("span", {}, h("span", { text: todo.text, style: todo.done ? "text-decoration:line-through;opacity:.6" : "" }), extra ? h("div", { class: "who", text: extra }) : null)));
      }
    }
    if (note.error) {
      el.append(h("div", { class: "hub-warn", role: "alert", text: note.error }));
    }
    el.append(button(busy ? "Gerando resumo…" : "Regerar resumo", () => void FlowBridge.summarize(note.id), "", true));
    return el;
  }

  function transcript(note: Note, empty: string): HTMLElement {
    const el = h("div", { style: "display:flex;flex-direction:column" });
    if (note.lines.length === 0 && empty) el.append(h("div", { class: "hub-note", text: empty }));
    for (const line of note.lines) {
      const s = note.speakers.find((x) => x.id === line.speaker);
      const name = s?.name ?? (line.speaker === "me" ? t("Me") : t("Others"));
      const color = s ? speakerColor(s.color) : line.speaker === "me" ? speakerColor(0) : speakerColor(1);
      el.append(h("div", { class: "hub-line" },
        h("span", { class: "ts", text: clock(line.start) }),
        h("div", {}, h("div", { class: "who", style: `color:${color}`, text: name }), h("div", { class: "said", text: line.text })),
      ));
    }
    return el;
  }

  drawList();
  drawDetail();
  const unlistenNotes = await onEvent<string>("notes-changed", () => void reload());
  const unlistenPersonas = await onEvent<unknown>("personas-changed", () => void reload());
  const timer = window.setInterval(() => {
    if (!state.recording) return;
    const b = listEl.querySelector<HTMLElement>("[data-ticking]");
    if (!b) return;
    const next = recordButton();
    next.style.width = "100%";
    b.replaceWith(next);
  }, 1000);
  ctx.onLeave = () => {
    left = true;
    unlistenNotes();
    unlistenPersonas();
    window.clearInterval(timer);
  };
}

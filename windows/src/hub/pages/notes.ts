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
import { button, icon, iconButton, segmented } from "../ui";
import { modelsControl } from "./flowsettings";

const lang = () => document.documentElement.lang || (LUMEN_WINDOWS ? "pt-BR" : undefined);
const pageAccent = () => (LUMEN_WINDOWS ? "#E99A3D" : "#3B9EFF");

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

/** Escolhe tinta escura (#0b0c0e) ou clara (#F3EEE6) conforme luminância relativa WCAG. */
function contrastInk(color: string): string {
  const m = color.trim().match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (!m) return "#0b0c0e";
  const raw =
    m[1].length === 3
      ? m[1]
          .split("")
          .map((c) => c + c)
          .join("")
      : m[1];
  const r = parseInt(raw.slice(0, 2), 16) / 255;
  const g = parseInt(raw.slice(2, 4), 16) / 255;
  const b = parseInt(raw.slice(4, 6), 16) / 255;
  const lin = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  const lum = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
  return lum > 0.179 ? "#0b0c0e" : "#F3EEE6";
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
  const ink = contrastInk(color);
  const circleStyle = `background:${color};width:${size}px;height:${size}px;border-radius:999px;display:inline-flex;align-items:center;justify-content:center;font:700 ${Math.max(11, Math.round(size * 0.45))}px var(--font);color:${ink};flex-shrink:0;overflow:hidden`;
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
    page.append(
      glowCard(
        pageAccent(),
        h("div", { class: "notes-import-title", text: "Modelo de voz opcional para ditado" }),
        h("div", {
          class: "hub-note",
          text: LUMEN_WINDOWS
            ? `Notas em texto já funcionam agora. Para ditar ideias por voz offline, baixe o Parakeet (${ctx.status.models.downloadMb ?? 0} MB).`
            : t("Coucou writes your calls on this computer with Parakeet, and tells the voices apart with a second small model. Both are downloaded once (about {0} MB), then everything works offline.", { 0: ctx.status.models.downloadMb }),
        }),
        modelsControl(ctx, () => ctx.navigate("notes")),
      ),
    );
  }

  let query = "";
  const search = h("input", {
    class: "hub-input notes-search-input",
    placeholder: t("Search"),
    "aria-label": t("Search"),
    type: "search",
  }) as HTMLInputElement;
  search.addEventListener("input", () => {
    query = search.value;
    drawList(true);
  });

  const quickTitle = h("input", {
    class: "hub-input notes-quick-title",
    placeholder: "Título opcional",
    "aria-label": "Título opcional da nota rápida",
  }) as HTMLInputElement;
  const quickText = h("textarea", {
    class: "hub-input notes-quick-text",
    rows: "2",
    placeholder: "Escreva uma ideia ou pensamento rápido…",
    "aria-label": "Texto da nova nota rápida",
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
      const b = button(
        `■  ${t("Stop")} · ${clock((Date.now() - (rec?.start ?? Date.now())) / 1000)}`,
        () => void FlowBridge.stopNote(),
        "danger",
      );
      b.classList.add("notes-rail-primary");
      b.dataset.ticking = "1";
      return b;
    }
    const b = button(
      "Ditar ideia",
      () => {
        startError = "";
        FlowBridge.startNote().then(
          (id) => {
            selected = id;
            showPersonas = false;
            void reload();
          },
          (err) => {
            startError = String(err).replace(/^Error:\s*/, "");
            drawList();
          },
        );
      },
      "primary",
    );
    b.classList.add("notes-rail-primary");
    b.prepend(h("span", { class: "hub-rec on-accent" }));
    return b;
  }

  function drawList(typing = false) {
    clear(listEl);

    const rb = recordButton();
    const activePersonasCount = personasState.personas.filter((p) => p.enabled).length;
    const personasBtn = button(
      showPersonas
        ? "← Voltar às notas"
        : `Personas (${activePersonasCount} ativa${activePersonasCount === 1 ? "" : "s"})`,
      () => {
        showPersonas = !showPersonas;
        drawList();
        drawDetail();
      },
      "",
      true,
    );
    personasBtn.classList.add("notes-personas-nav");
    if (showPersonas) personasBtn.classList.add("on");
    personasBtn.setAttribute("aria-pressed", String(showPersonas));

    listEl.append(h("div", { class: "notes-rail-actions" }, rb, personasBtn));

    const saveBtn = button(
      savingText ? "Salvando…" : "Salvar ideia em texto",
      () => void saveQuickIdea(),
      "",
      true,
    );
    saveBtn.classList.add("notes-save-btn");

    listEl.append(
      h(
        "div",
        { class: "notes-composer" },
        h(
          "div",
          { class: "notes-composer-head" },
          h("div", { class: "hub-group", text: "Nova nota rápida" }),
          h("span", { class: "notes-kbd-hint", text: "Ctrl+Enter" }),
        ),
        quickTitle,
        quickText,
        quickError ? h("div", { class: "hub-warn", role: "alert", text: quickError }) : null,
        saveBtn,
      ),
    );

    if (state.notes.length > 0) {
      listEl.append(h("div", { class: "notes-search-wrap" }, icon("search"), search));
    }
    if (typing) search.focus();
    if (startError) listEl.append(h("div", { class: "hub-warn", role: "alert", text: startError }));

    const itemsWrap = h("div", { class: "notes-items" });
    if (state.notes.length === 0) {
      itemsWrap.append(
        h("div", {
          class: "hub-note",
          text: "Suas ideias e anotações aparecem aqui. Dite no microfone ou salve em texto.",
        }),
      );
    } else {
      let day = "";
      const filtered = state.notes.filter((x) => noteMatches(x, query));
      if (filtered.length === 0) {
        itemsWrap.append(h("div", { class: "hub-note", text: "Nenhuma anotação corresponde à busca." }));
      }
      for (const n of filtered) {
        const d = new Date(n.start).toDateString();
        if (d !== day) {
          day = d;
          const isToday = d === new Date().toDateString();
          itemsWrap.append(
            h("div", {
              class: "hub-group notes-day-label",
              text: isToday ? t("Today") : new Date(n.start).toLocaleDateString(lang(), { day: "numeric", month: "long" }),
            }),
          );
        }
        const commentCount = n.comments?.length ?? 0;
        const meta =
          n.status === "recording"
            ? t("Recording…")
            : state.working.includes(n.id) || n.status === "processing"
              ? t("writing…")
              : n.status === "failed"
                ? t("failed")
                : `${new Date(n.start).toLocaleTimeString(lang(), { hour: "2-digit", minute: "2-digit" })} · ${commentCount} coment.`;
        const isCurrent = n.id === selected && !showPersonas;
        const item = h(
          "button",
          {
            type: "button",
            class: isCurrent ? "hub-note-item on" : "hub-note-item",
            "aria-current": isCurrent ? "true" : undefined,
            onclick: () => {
              selected = n.id;
              showPersonas = false;
              replyToId = null;
              commentError = "";
              drawList();
              drawDetail();
            },
          },
          h("span", { class: "t" }, h("span", { class: "t-label", text: n.title })),
          h("span", { class: "m", text: meta }),
        );
        if (n.status === "recording") item.querySelector(".m")!.prepend(h("span", { class: "hub-rec inline" }));
        itemsWrap.append(item);
      }
    }
    listEl.append(itemsWrap);

    listEl.append(
      h("div", {
        class: "notes-rail-foot",
        text: "Tudo fica salvo localmente. Notas por voz usam apenas o microfone e seu provedor ativo.",
      }),
    );
  }

  function renderPersonasManager() {
    clear(detail);
    const settings: CommentSettings = personasState.settings;
    const enableBox = h("input", {
      type: "checkbox",
      class: "notes-checkbox",
    }) as HTMLInputElement;
    enableBox.checked = settings.enableCharacterComment;
    const maxInput = h("input", {
      class: "hub-input notes-max-input",
      type: "number",
      min: "1",
      max: "15",
      value: String(settings.maxCommentCharacters),
      "aria-label": "Máximo de personas por nota",
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
      class: "hub-input notes-import-textarea",
      rows: "7",
      placeholder: "Cole JSON ou .yaml da Segunda Mente (junte vários docs com ---) ou 3 partes QwenPaw (identity, userProfile, rules)…",
      "aria-label": "Conteúdo JSON ou YAML das personas",
    }) as HTMLTextAreaElement;
    jsonBox.value = personaDraft;
    jsonBox.addEventListener("input", () => {
      personaDraft = jsonBox.value;
    });

    const fileInput = h("input", {
      type: "file",
      accept: ".json,.yaml,.yml,application/json,text/yaml",
      multiple: true,
      class: "notes-file-input",
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
    const sampleBtn = button(
      "Preencher exemplo sintético",
      () => {
        personaDraft = SYNTHETIC_PERSONA_SAMPLE;
        jsonBox.value = personaDraft;
      },
      "",
      true,
    );
    for (const b of [importBtn, chooseBtn, syncAdbBtn, resetBtn, sampleBtn]) b.disabled = busy;

    const backBtn = button(
      "← Voltar à nota",
      () => {
        showPersonas = false;
        drawList();
        drawDetail();
      },
      "",
      true,
    );
    backBtn.classList.add("notes-personas-back");

    detail.append(
      h(
        "div",
        { class: "notes-personas-head" },
        h("h2", { text: "Personas e regras de comentário" }),
        backBtn,
      ),
      h(
        "div",
        { class: "hub-card notes-rules-card" },
        h(
          "label",
          { class: "notes-check-row" },
          enableBox,
          h("span", { text: "Permitir comentários automáticos após criar ou ditar uma nota" }),
        ),
        h(
          "div",
          { class: "notes-rules-limit" },
          h("span", { class: "hub-note", text: "Máximo de personas por nota (até 15 no chat em grupo, se pertinente):" }),
          maxInput,
          h("span", { class: "notes-rules-hint", text: "Dica: @nome sempre responde mesmo se o automático estiver desligado." }),
        ),
      ),
      h(
        "div",
        { class: "hub-card notes-import-card" },
        h("div", { class: "notes-import-title", text: "Importar ou sincronizar personas (JSON ou .yaml da Segunda Mente)" }),
        fileInput,
        jsonBox,
        h(
          "div",
          { class: "notes-import-actions" },
          importBtn,
          chooseBtn,
          syncAdbBtn,
          resetBtn,
          sampleBtn,
        ),
        personaFeedback ? h("div", { class: "hub-note notes-personas-feedback", role: "status", text: personaFeedback }) : null,
        personaError ? h("div", { class: "hub-warn", role: "alert", text: personaError }) : null,
      ),
    );

    if (personasState.personas.length === 0) {
      detail.append(h("div", { class: "hub-note", text: "Nenhuma persona importada ainda." }));
      return;
    }

    const grid = h("div", { class: "notes-personas-grid" });
    for (const p of personasState.personas) {
      const cardEl = h(
        "div",
        { class: p.enabled ? "hub-card notes-persona-card" : "hub-card notes-persona-card is-off" },
        h(
          "div",
          { class: "notes-persona-top" },
          h(
            "div",
            { class: "notes-persona-ident" },
            personaAvatar(p.name, p.color, p.avatar, 36),
            h(
              "div",
              { class: "notes-persona-titles" },
              h(
                "div",
                { class: "notes-persona-name-row" },
                h("strong", { text: `${p.name} (@${p.id})` }),
                p.isPrimaryCompanion ? h("span", { class: "notes-persona-primary", text: "Principal" }) : null,
                !p.enabled ? h("span", { class: "notes-persona-inactive", text: "Desativada" }) : null,
              ),
              p.role ? h("div", { class: "hub-note", text: p.role }) : null,
            ),
          ),
        ),
        p.tags.length
          ? h(
              "div",
              { class: "notes-persona-tags" },
              ...p.tags.map((tag) => h("span", { class: "notes-tag-chip", text: tag })),
            )
          : null,
        p.interestFilter || p.identity || p.rules
          ? h(
              "div",
              { class: "notes-persona-fields" },
              p.interestFilter ? h("div", { class: "hub-note", text: `Foco: ${p.interestFilter}` }) : null,
              p.identity ? h("div", { class: "hub-note", text: `Identidade: ${p.identity}` }) : null,
              p.rules ? h("div", { class: "hub-note", text: `Regras: ${p.rules}` }) : null,
            )
          : null,
        p.firstMessage?.trim()
          ? h("div", {
              class: "notes-persona-quote",
              text: `Primeira fala: ${p.firstMessage.trim()}`,
            })
          : null,
        p.mesExample?.trim()
          ? h(
              "details",
              { class: "hub-note notes-persona-example" },
              h("summary", { text: "Exemplo de diálogo" }),
              h("div", {
                class: "notes-persona-example-body",
                text: p.mesExample.trim(),
              }),
            )
          : null,
        h(
          "div",
          { class: "notes-persona-actions" },
          button(
            p.enabled ? "Desativar" : "Ativar",
            async () => {
              personaError = "";
              try {
                personasState = await FlowBridge.togglePersona(p.id, !p.enabled);
                drawList();
              } catch (err) {
                personaError = String(err).replace(/^Error:\s*/, "");
              }
              renderPersonasManager();
            },
            "",
            true,
          ),
          !p.isPrimaryCompanion
            ? button(
                "Definir principal",
                async () => {
                  personaError = "";
                  try {
                    personasState = await FlowBridge.setPrimaryPersona(p.id);
                  } catch (err) {
                    personaError = String(err).replace(/^Error:\s*/, "");
                  }
                  renderPersonasManager();
                },
                "",
                true,
              )
            : null,
          button(
            t("Delete"),
            async () => {
              personaError = "";
              try {
                personasState = await FlowBridge.deletePersona(p.id);
                drawList();
              } catch (err) {
                personaError = String(err).replace(/^Error:\s*/, "");
              }
              renderPersonasManager();
            },
            "danger",
            true,
          ),
        ),
      );
      grid.append(cardEl);
    }
    detail.append(grid);
  }

  function renderComments(note: Note): HTMLElement {
    const section = h("div", { class: "hub-card notes-comments-sheet" });
    const comments = note.comments ?? [];
    const busy = state.working.includes(note.id);
    const activeCount = personasState.personas.filter((p) => p.enabled).length;

    const header = h(
      "div",
      { class: "notes-comments-head" },
      h("div", { class: "notes-comments-title", text: `Comentários das personas (${comments.length})` }),
      h(
        "div",
        { class: "notes-comments-actions" },
        button(
          busy ? "Gerando…" : "Gerar comentários",
          async () => {
            commentError = "";
            try {
              await FlowBridge.generateNoteComments(note.id);
              await reload();
            } catch (err) {
              commentError = String(err).replace(/^Error:\s*/, "");
              drawDetail();
            }
          },
          "",
          true,
        ),
        button(
          "Gerenciar personas",
          () => {
            showPersonas = true;
            drawList();
            drawDetail();
          },
          "",
          true,
        ),
      ),
    );
    section.append(header);

    if (commentError) {
      section.append(h("div", { class: "hub-warn", role: "alert", text: commentError }));
    }

    if (comments.length === 0) {
      const hint =
        activeCount === 0
          ? "Importe e ative uma persona para receber comentários automáticos nas suas notas."
          : "Nenhum comentário ainda. Clique em Gerar comentários ou mencione @persona abaixo.";
      section.append(h("div", { class: "hub-note", text: hint }));
    } else {
      const byId = new Map(comments.map((c) => [c.id, c]));
      const thread = h("div", { class: "notes-thread" });
      for (const c of comments) {
        const persona = personasState.personas.find((p) => p.id === c.characterId);
        const color = c.authorKind === "user" ? pageAccent() : (persona?.color ?? "#A78BFA");
        const parent = c.replyToId ? byId.get(c.replyToId) : undefined;
        thread.append(
          h(
            "div",
            { class: parent ? "notes-comment is-reply" : "notes-comment" },
            h(
              "div",
              { class: "notes-comment-top" },
              h(
                "div",
                { class: "notes-comment-author-wrap" },
                personaAvatar(c.characterName, color, c.authorKind === "user" ? null : persona?.avatar, 24),
                h("strong", { class: "notes-comment-author", text: c.characterName }),
                parent ? h("span", { class: "notes-comment-reply-tag", text: `↳ em resposta a ${parent.characterName}` }) : null,
              ),
              c.authorKind !== "user"
                ? button(
                    "Responder",
                    () => {
                      replyToId = c.id;
                      drawDetail();
                    },
                    "",
                    true,
                  )
                : null,
            ),
            h("div", { class: "notes-comment-body", text: c.content }),
          ),
        );
      }
      section.append(thread);
    }

    const replyTarget = replyToId ? comments.find((c) => c.id === replyToId) : undefined;
    if (replyTarget) {
      section.append(
        h(
          "div",
          { class: "notes-replying-bar" },
          h("span", { text: `Respondendo a ${replyTarget.characterName}` }),
          button(
            "Cancelar resposta",
            () => {
              replyToId = null;
              drawDetail();
            },
            "",
            true,
          ),
        ),
      );
    }

    const replyInput = h("input", {
      class: "hub-input notes-comment-input",
      placeholder: activeCount > 0 ? "Comentar ou mencionar @persona…" : "Adicionar observação nesta nota…",
      "aria-label": "Comentar ou mencionar persona",
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
      h(
        "div",
        { class: "notes-comment-bar" },
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
      detail.append(
        h(
          "div",
          { class: "hub-card notes-doc-sheet" },
          emptyState("Suas ideias e pensamentos aparecerão aqui. Dite pelo microfone ou registre em texto.", "sleeping", pageAccent()),
        ),
      );
      return;
    }

    const docSheet = h("div", { class: "hub-card notes-doc-sheet" });
    detail.append(docSheet);

    const title = h("h2", { class: "notes-doc-title", text: note.title, title: t("Edit") });
    const startRename = () => {
      const input = h("input", {
        class: "hub-input notes-doc-title-input",
        "aria-label": t("Edit"),
      }) as HTMLInputElement;
      input.value = note.title;
      const done = () => void FlowBridge.renameNote(note.id, input.value);
      input.addEventListener("keydown", (e) => {
        if (e.key === "Enter") done();
        if (e.key === "Escape") drawDetail();
      });
      input.addEventListener("blur", done);
      titleRow.replaceWith(input);
      input.focus();
      input.select();
    };
    title.addEventListener("click", startRename);
    const renameBtn = iconButton("pencil", t("Edit"), startRename);
    renameBtn.classList.add("notes-rename-btn");
    const titleRow = h("div", { class: "notes-doc-title-row" }, title, renameBtn);

    const when = new Date(note.start).toLocaleString(lang(), {
      weekday: "long",
      day: "numeric",
      month: "long",
      hour: "2-digit",
      minute: "2-digit",
    });
    const copied = h("span", { class: "notes-copy-status", role: "status", "aria-live": "polite" });
    const tools = h("div", { class: "notes-doc-actions" });
    const confirm = h("div", {});
    if (note.status === "ready") {
      const noteId = note.id;
      tools.append(
        button(
          t("Copy as Markdown"),
          async () => {
            const md = await FlowBridge.markdown(note.id);
            if (md && (await FlowBridge.copy(md))) {
              copied.textContent = t("Copied");
              setTimeout(() => (copied.textContent = ""), 1500);
            }
          },
          "",
          true,
        ),
      );
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
      tools.append(
        iconButton("trash", t("Delete"), () => {
          clear(confirm);
          confirm.append(
            h(
              "div",
              { class: "hub-warn notes-confirm-banner", role: "alert" },
              h("span", { class: "notes-confirm-text", text: "Excluir esta anotação e seus comentários?" }),
              button(t("Delete"), () => void FlowBridge.deleteNote(note.id), "danger", true),
              button(t("Cancel"), () => clear(confirm), "", true),
            ),
          );
        }),
      );
    }
    tools.append(copied);

    docSheet.append(
      h(
        "div",
        { class: "notes-doc-header" },
        h(
          "div",
          { class: "notes-doc-title-col" },
          titleRow,
          h("div", { class: "notes-doc-meta", text: note.app ? `${when} · ${note.app}` : when }),
        ),
        tools,
      ),
      confirm,
    );
    if (exportResult?.id === note.id) {
      docSheet.append(
        h("div", {
          class: exportResult.error ? "hub-warn notes-export-status" : "hub-note notes-export-status notes-export-ok",
          role: exportResult.error ? "alert" : "status",
          text: exportResult.error ?? t("Saved as {name}", { name: exportResult.path ?? "" }),
        }),
      );
    }

    if (note.speakers.length) {
      const people = h("div", { class: "hub-speakers" });
      for (const s of note.speakers) {
        const chip = h(
          "button",
          { type: "button", class: "hub-speaker", title: t("Who is it?") },
          h("span", { class: "av", style: `background:${speakerColor(s.color)}`, text: s.name.slice(0, 1).toUpperCase() }),
          h("span", { text: s.name }),
        );
        chip.addEventListener("click", () => {
          const input = h("input", {
            class: "hub-input notes-speaker-input",
            placeholder: t("Name"),
            "aria-label": t("Who is it?"),
          }) as HTMLInputElement;
          input.value = s.name;
          const done = () => void FlowBridge.renameSpeaker(note.id, s.id, input.value);
          input.addEventListener("keydown", (e) => {
            if (e.key === "Enter") done();
            if (e.key === "Escape") drawDetail();
          });
          input.addEventListener("blur", done);
          chip.replaceWith(input);
          input.focus();
          input.select();
        });
        people.append(chip);
      }
      docSheet.append(people);
    }

    if (note.status === "recording") {
      docSheet.append(h("div", { class: "notes-recording-banner" }, h("span", { class: "hub-rec" }), h("b", { text: t("Live") })));
      docSheet.append(transcript(note, t("The words show up here as people speak.")));
      return;
    }
    if (note.status === "processing" || (state.working.includes(note.id) && note.lines.length === 0)) {
      docSheet.append(emptyState("Transcrevendo e organizando sua ideia…", "working", pageAccent()));
      return;
    }
    if (note.status === "failed") {
      docSheet.append(h("div", { class: "hub-warn", role: "alert", text: `Não foi possível processar esta gravação. ${note.error ?? ""}` }));
      docSheet.append(button(t("Retry"), () => void FlowBridge.retryNote(note.id)));
      return;
    }
    if (note.lines.length === 0) {
      docSheet.append(emptyState("Nenhuma fala foi captada nesta gravação.", "idle", pageAccent()));
      return;
    }

    const tabs = segmented<"summary" | "transcript">(
      [
        ["summary", "Resumo & Personas"],
        ["transcript", "Texto / Transcrição"],
      ],
      tab,
      (next) => {
        tab = next;
        drawDetail();
      },
    );
    docSheet.append(h("div", { class: "notes-doc-tabs" }, tabs));
    if (tab === "summary") {
      docSheet.append(summary(note));
    } else {
      docSheet.append(transcript(note, ""));
    }
    detail.append(renderComments(note));
  }

  let todoTaskError = "";

  function convertTodo(noteId: string, todoId: string): void {
    todoTaskError = "";
    FlowBridge.convertTodoToTask(noteId, todoId).catch((err) => {
      todoTaskError = String(err).replace(/^Error:\s*/, "");
      drawDetail();
    });
  }

  function summary(note: Note): HTMLElement {
    const el = h("div", { class: "notes-summary" });
    const busy = state.working.includes(note.id);
    if (!note.summary && note.keyPoints.length === 0 && note.todos.length === 0) {
      const rawText = note.lines.map((l) => l.text).join("\n");
      if (rawText) {
        el.append(h("p", { class: "notes-summary-prose", text: rawText }));
      } else {
        el.append(h("div", { class: "hub-note", text: busy ? "Gerando resumo…" : "Ainda sem resumo automático." }));
      }
      if (note.error) {
        el.append(h("div", { class: "hub-warn", role: "alert", text: note.error }));
      }
      if (!busy) {
        el.append(h("div", { class: "notes-summary-foot" }, button("Gerar resumo", () => void FlowBridge.summarize(note.id), "primary", true)));
      }
      return el;
    }
    if (note.summary) {
      el.append(h("p", { class: "notes-summary-prose", text: note.summary }));
    }
    if (note.keyPoints.length) {
      const ul = h("ul", { class: "notes-points" });
      for (const p of note.keyPoints) ul.append(h("li", { text: p }));
      el.append(h("div", { class: "notes-section-block" }, h("div", { class: "hub-label", text: "Pontos principais" }), ul));
    }
    if (note.todos.length) {
      const todosWrap = h("div", { class: "notes-todos" });
      for (const todo of note.todos) {
        const box = h("input", { type: "checkbox" }) as HTMLInputElement;
        box.checked = todo.done;
        box.addEventListener("change", () => void FlowBridge.toggleTodo(note.id, todo.id));
        const extra = [todo.owner, todo.due].filter(Boolean).join(" · ");
        const linked = Boolean(todo.taskId);
        const virar = button(linked ? "Virou tarefa" : "Virar tarefa", () => convertTodo(note.id, todo.id), "", true);
        virar.classList.add("notes-todo-task");
        virar.disabled = linked;
        todosWrap.append(
          h(
            "label",
            { class: "hub-todo" },
            box,
            h(
              "span",
              {},
              h("span", { class: todo.done ? "notes-todo-text done" : "notes-todo-text", text: todo.text }),
              extra ? h("div", { class: "who", text: extra }) : null,
            ),
            virar,
          ),
        );
      }
      el.append(h("div", { class: "notes-section-block" }, h("div", { class: "hub-label", text: "Ações / To-do" }), todosWrap));
      if (todoTaskError) el.append(h("div", { class: "hub-warn", role: "alert", text: todoTaskError }));
    }
    if (note.error) {
      el.append(h("div", { class: "hub-warn", role: "alert", text: note.error }));
    }
    el.append(h("div", { class: "notes-summary-foot" }, button(busy ? "Gerando resumo…" : "Regerar resumo", () => void FlowBridge.summarize(note.id), "", true)));
    return el;
  }

  function transcript(note: Note, empty: string): HTMLElement {
    const el = h("div", { class: "notes-transcript" });
    if (note.lines.length === 0 && empty) el.append(h("div", { class: "hub-note", text: empty }));
    for (const line of note.lines) {
      const s = note.speakers.find((x) => x.id === line.speaker);
      const name = s?.name ?? (line.speaker === "me" ? t("Me") : t("Others"));
      const color = s ? speakerColor(s.color) : line.speaker === "me" ? speakerColor(0) : speakerColor(1);
      el.append(
        h(
          "div",
          { class: "hub-line" },
          h("span", { class: "ts", text: clock(line.start) }),
          h(
            "div",
            {},
            h("div", { class: "who", style: `color:${color}`, text: name }),
            h("div", { class: "said", text: line.text }),
          ),
        ),
      );
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
    b.replaceWith(next);
  }, 1000);
  ctx.onLeave = () => {
    left = true;
    unlistenNotes();
    unlistenPersonas();
    window.clearInterval(timer);
  };
}

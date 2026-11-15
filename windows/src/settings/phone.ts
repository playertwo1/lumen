// Settings → Android phone: the switch for the phone link and the code that
// pairs a phone with this computer. Off until the user turns it on.

import { Bridge, type PhoneChatStatus, type PhoneLinkPairing, type PhoneLinkStatus, type PhoneRelayState, type PhoneRelayStatus } from "../core/bridge";
import { modelId, modelsOf, toggleModel, usableProviders } from "../core/phone-chat";
import { PROVIDERS, type ProviderDef } from "../core/providers";
import type { Settings } from "../core/state";
import { h } from "../views/dom";
import { t } from "../i18n/i18n";

type Toggle = (on: boolean, onChange: (v: boolean) => void) => HTMLElement;

function sectionDivider(): HTMLElement {
  return h("div", { class: "phone-divider" });
}

/**
 * The relay block ("Away from home Wi-Fi") is hidden until a relay is ready to use: the code and its tests stay.
 * Testers who start Coucou with COUCOU_RELAY_SETTINGS=1 see it (docs/RELAY_DEPLOY.md), and so does anyone whose relay
 * is already on. Nothing else changes.
 */
function relaySlot(makeToggle: Toggle, refreshPairing: () => void): HTMLElement {
  const slot = h("div", { class: "phone-relay-slot" });
  void Bridge.phoneRelayStatus().then((status) => {
    if (status?.shown) slot.replaceWith(relayBlock(makeToggle, refreshPairing));
  });
  return slot;
}

export function phoneSection(on: boolean, makeToggle: Toggle, settings: Settings): HTMLElement {
  const detail = h("div", { class: "phone-detail" });
  const note = h("div", { class: "notice err phone-error", hidden: true });
  /** Where the pairing code is drawn, so showing it never hides the switches below. */
  const pairingBox = h("div", { class: "phone-pairing-wrap" });
  const statusPill = h("span", { class: "phone-live-pill", text: on ? "Ativo" : "Desligado" });

  const showError = (message: string) => {
    note.hidden = false;
    note.textContent = t("Could not turn it on: {error}", { error: message });
  };

  /** The code itself, which lets a phone in: shown only when asked for. */
  const reveal = async (status: PhoneLinkStatus | null, newCode: boolean) => {
    try {
      const pairing = newCode ? await Bridge.phoneLinkNewPairing() : await Bridge.phoneLinkPairing();
      pairingBox.replaceChildren(...pairingRows(pairing, () => void reveal(status, true)));
    } catch (err) {
      showError(String(err));
    }
  };

  /** After the relay settings change, a code that is on screen would be stale: show the current one. */
  const refreshPairing = async () => {
    if (pairingBox.childElementCount === 0) return;
    try {
      pairingBox.replaceChildren(...pairingRows(await Bridge.phoneLinkPairing(), () => void reveal(null, true)));
    } catch (err) {
      showError(String(err));
    }
  };

  /** Draws what is below the switch from the link's state. */
  const draw = (status: PhoneLinkStatus | null) => {
    detail.replaceChildren();
    pairingBox.replaceChildren();
    note.textContent = "";
    note.hidden = true;
    if (status?.error) showError(status.error);
    const running = Boolean(status?.running);
    switchEl.classList.toggle("on", running);
    switchEl.setAttribute("aria-pressed", String(running));
    if (!status?.running) {
      statusPill.className = "phone-live-pill";
      statusPill.textContent = "Desligado";
      return;
    }

    const address = status.host ? `${status.host}:${status.port}` : `:${status.port}`;
    statusPill.className = status.clients > 0 ? "phone-live-pill ok" : "phone-live-pill active";
    statusPill.textContent = status.clients > 0 ? `Conectado · ${address}` : `Aguardando · ${address}`;

    const show = h("button", { class: "primary", text: t("Show pairing code") }) as HTMLButtonElement;
    show.addEventListener("click", () => {
      show.hidden = true;
      void reveal(status, false);
    });
    clients.textContent = t("Phones connected: {count}", { count: status.clients });

    const pairingCard = h(
      "section",
      { class: "phone-card" },
      h(
        "div",
        { class: "phone-card-head" },
        h(
          "div",
          { class: "phone-card-title-col" },
          h("h2", { text: "Pareamento na rede local" }),
          clients,
        ),
        show,
      ),
      pairingBox,
      sectionDivider(),
      h(
        "div",
        { class: "phone-meta-notes" },
        h("div", { class: "hint", text: t("The phone connects to {address}", { address }) }),
        h("div", { class: "hint", text: t("If Windows asks about the firewall, allow Coucou on private networks. The phone and this computer must be on the same Wi-Fi.") }),
        h("div", { class: "hint", text: t("Your phone finds this computer again by itself when the network changes. It is announced on your local network only while this switch is on. Some hotspots and guest networks block this.") }),
      ),
    );

    const permissionsCard = h(
      "section",
      { class: "phone-card" },
      h(
        "div",
        { class: "phone-card-head" },
        h(
          "div",
          { class: "phone-card-title-col" },
          h("h2", { text: "Permissões e contexto compartilhado" }),
          h("div", { class: "hint", text: "Escolha exatamente o que o aplicativo Android pode visualizar ou responder." }),
        ),
      ),
      answersBlock(makeToggle),
      sectionDivider(),
      detailsBlock(makeToggle),
      sectionDivider(),
      diffsBlock(makeToggle),
      sectionDivider(),
      usageBlock(makeToggle),
      sectionDivider(),
      recapBlock(makeToggle),
    );

    const extrasCard = h(
      "section",
      { class: "phone-card" },
      h("h2", { text: "Serviços e chat remoto" }),
      h("div", { class: "phone-extras-grid" }, servicesBlock(), chatBlock(makeToggle, settings)),
    );

    const leftCol = h(
      "div",
      { class: "phone-col" },
      pairingCard,
      relaySlot(makeToggle, () => void refreshPairing()),
    );
    const rightCol = h(
      "div",
      { class: "phone-col" },
      permissionsCard,
    );

    detail.append(h("div", { class: "phone-workbench" }, leftCol, rightCol), extrasCard);
  };

  /** Kept up to date while the window is open and the link is on; nothing runs otherwise. */
  const clients = h("div", { class: "hint phone-clients-count" });
  const watch = window.setInterval(() => {
    if (!section.isConnected) return window.clearInterval(watch);
    if (document.visibilityState !== "visible" || !clients.isConnected) return;
    void Bridge.phoneLinkStatus().then((s) => {
      if (!s?.running) return;
      clients.textContent = t("Phones connected: {count}", { count: s.clients });
      const addr = s.host ? `${s.host}:${s.port}` : `:${s.port}`;
      statusPill.className = s.clients > 0 ? "phone-live-pill ok" : "phone-live-pill active";
      statusPill.textContent = s.clients > 0 ? `Conectado · ${addr}` : `Aguardando · ${addr}`;
    });
  }, 5000);

  const switchEl = makeToggle(on, (next) => {
    void (async () => {
      try {
        draw(await Bridge.phoneLinkSetEnabled(next));
      } catch (err) {
        // It could not start: the switch goes back off and says why.
        switchEl.classList.remove("on");
        switchEl.setAttribute("aria-pressed", "false");
        detail.replaceChildren();
        statusPill.className = "phone-live-pill";
        statusPill.textContent = "Desligado";
        showError(String(err));
      }
    })();
  });

  void Bridge.phoneLinkStatus().then(draw);

  const masterCard = h(
    "section",
    { class: "phone-card phone-master-card" },
    h(
      "div",
      { class: "phone-master-head" },
      h(
        "div",
        { class: "phone-card-title-col" },
        h("h2", {}, h("span", { text: t("Android phone") }), statusPill),
        h("div", { class: "hint", text: t("Send your agents' sessions to Coucou for Android and answer permission requests from the phone. Off by default; it only listens on your local network.") }),
        h("div", { class: "hint", text: t("Allow on the phone asks for the fingerprint or the screen lock. Deny does not.") }),
      ),
      h(
        "div",
        { class: "phone-master-switch" },
        h("label", { text: t("Phone link") }),
        switchEl,
      ),
    ),
    note,
  );

  const section = h("div", { class: "phone-page" }, masterCard, detail);
  return section;
}

/** What the status line says about the relay. */
export function relayStateText(state: PhoneRelayState): string {
  switch (state) {
    case "connecting": return t("Reaching the relay…");
    case "waiting": return t("Connected to the relay, waiting for the phone.");
    case "linked": return t("The phone is connected through the relay.");
    case "accessRefused": return t("The relay refused the access key.");
    case "unreachable": return t("The relay cannot be reached. Trying again.");
    case "roomTaken": return t("This pairing is in use somewhere else. Press Pair again.");
    case "tooMany": return t("The relay is limiting connections from here. Trying again later.");
    default: return t("Off");
  }
}

/**
 * "Away from home Wi-Fi": reach the phone through a relay the user deployed on their own account. Off until the user
 * enters the address and the access key and turns it on. The access key is write-only: it goes to the system keystore
 * and the window never gets it back. Rust re-reads the switch for every connection.
 */
function relayBlock(makeToggle: Toggle, changed: () => void): HTMLElement {
  const note = h("div", { class: "hint phone-relay-note" });
  const state = h("div", { class: "phone-relay-state" });
  const stateBadge = h("span", { class: "phone-live-pill", text: t("Off") });
  const address = h("input", { type: "text", class: "phone-mono-input", placeholder: "wss://", autocomplete: "off", spellcheck: "false" }) as HTMLInputElement;
  const access = h("input", { type: "password", class: "phone-mono-input", autocomplete: "off", spellcheck: "false" }) as HTMLInputElement;
  let current: PhoneRelayStatus = { enabled: false, url: "", hasAccess: false, state: "off", shown: true };

  const show = (status: PhoneRelayStatus | null) => {
    if (!status) return;
    current = status;
    switchEl.classList.toggle("on", status.enabled);
    switchEl.setAttribute("aria-pressed", String(status.enabled));
    if (document.activeElement !== address) address.value = status.url;
    access.placeholder = status.hasAccess ? "••••••••" : "";
    const text = status.enabled ? relayStateText(status.state) : "";
    state.textContent = text;
    if (!status.enabled) {
      stateBadge.className = "phone-live-pill";
      stateBadge.textContent = t("Off");
    } else if (status.state === "linked") {
      stateBadge.className = "phone-live-pill ok";
      stateBadge.textContent = "Conectado";
    } else if (status.state === "accessRefused" || status.state === "roomTaken") {
      stateBadge.className = "phone-live-pill err";
      stateBadge.textContent = "Atenção";
    } else {
      stateBadge.className = "phone-live-pill active";
      stateBadge.textContent = "Ativo";
    }
  };
  const fail = (err: unknown) => {
    note.textContent = String(err);
    switchEl.classList.remove("on");
    switchEl.setAttribute("aria-pressed", "false");
  };
  const apply = async (enabled: boolean) => {
    note.textContent = "";
    try {
      show(await Bridge.phoneRelaySet(enabled, address.value));
      changed();
    } catch (err) {
      fail(err);
    }
  };
  const switchEl = makeToggle(false, (next) => void apply(next));
  const saveAddress = h("button", { text: t("Save address") });
  saveAddress.addEventListener("click", () => void apply(current.enabled));
  const saveKey = h("button", { text: t("Save key") });
  saveKey.addEventListener("click", () => {
    void (async () => {
      note.textContent = "";
      try {
        const next = await Bridge.phoneRelaySetAccess(access.value);
        access.value = ""; // never kept on the page
        show(next);
        changed();
      } catch (err) {
        note.textContent = String(err);
      }
    })();
  });

  void Bridge.phoneRelayStatus().then(show);
  // Kept up to date while the window is open; nothing runs while it is hidden.
  const watch = window.setInterval(() => {
    if (!state.isConnected) return window.clearInterval(watch);
    if (document.visibilityState !== "visible") return;
    void Bridge.phoneRelayStatus().then(show);
  }, 5000);

  return h(
    "section",
    { class: "phone-card" },
    h(
      "div",
      { class: "phone-card-head" },
      h("h2", {}, h("span", { text: t("Away from home Wi-Fi") }), stateBadge),
      switchEl,
    ),
    h("div", { class: "hint", text: t("Reach this computer from your phone on mobile data or another network, through a relay you deployed yourself. Everything is encrypted end to end: the relay only forwards data it cannot read. Off by default.") }),
    h(
      "div",
      { class: "phone-field-group" },
      h("label", { class: "phone-field-label", text: t("Relay address") }),
      h("div", { class: "phone-input-row" }, address, saveAddress),
    ),
    h(
      "div",
      { class: "phone-field-group" },
      h("label", { class: "phone-field-label", text: t("Relay access key") }),
      h("div", { class: "phone-input-row" }, access, saveKey),
      h("div", { class: "hint", text: t("Paste the access key you set when you deployed the relay. It is kept in the system keystore and never shown again.") }),
    ),
    state,
    note,
  );
}

/**
 * Session details for the phone (the steps, the last line, the folder name, the colour): off until the
 * user turns it on. The phone is told only if it asks; Rust re-reads this switch for every connection.
 */
function detailsBlock(makeToggle: Toggle): HTMLElement {
  const note = h("div", { class: "hint" });
  const switchEl = makeToggle(false, (next) => {
    void (async () => {
      note.textContent = "";
      try {
        await Bridge.phoneDetailsSetEnabled(next);
      } catch (err) {
        switchEl.classList.remove("on");
        switchEl.setAttribute("aria-pressed", "false");
        note.textContent = String(err);
      }
    })();
  });
  // The switch shows what Rust says, not what the page remembers.
  void Bridge.phoneDetailsStatus().then((status) => {
    if (!status) return;
    switchEl.classList.toggle("on", status.enabled);
    switchEl.setAttribute("aria-pressed", String(status.enabled));
  });
  return h(
    "div",
    { class: "phone-perm-item" },
    h(
      "div",
      { class: "phone-perm-row" },
      h(
        "div",
        { class: "phone-perm-copy" },
        h("label", { text: t("Show session details on the phone") }),
        h("div", { class: "hint", text: t("Sends the last steps (as shown on the island, so they can include commands), the final line, the project's folder name and the colour. Never a path.") }),
      ),
      switchEl,
    ),
    note,
  );
}

/**
 * Answering the questions Claude Code asks, from the phone: off until the user turns it on. The phone shows the question
 * and its options; its screen lock confirms every answer. Rust re-reads this switch for every connection.
 */
function answersBlock(makeToggle: Toggle): HTMLElement {
  const note = h("div", { class: "hint" });
  const switchEl = makeToggle(false, (next) => {
    void (async () => {
      note.textContent = "";
      try {
        await Bridge.phoneAnswersSetEnabled(next);
      } catch (err) {
        switchEl.classList.remove("on");
        switchEl.setAttribute("aria-pressed", "false");
        note.textContent = String(err);
      }
    })();
  });
  void Bridge.phoneAnswersStatus().then((status) => {
    if (!status) return;
    switchEl.classList.toggle("on", status.enabled);
    switchEl.setAttribute("aria-pressed", String(status.enabled));
  });
  return h(
    "div",
    { class: "phone-perm-item" },
    h(
      "div",
      { class: "phone-perm-row" },
      h(
        "div",
        { class: "phone-perm-copy" },
        h("label", { text: t("Let the phone answer Claude Code's questions") }),
        h("div", { class: "hint", text: t("The phone shows each question with its options. Every answer is confirmed with the phone's fingerprint or screen lock.") }),
      ),
      switchEl,
    ),
    note,
  );
}

/** The service pills the phone may show, with the names the pills carry (brand names, not translated). */
const PHONE_SERVICES: [string, string][] = [
  ["integration_stripe", "Stripe"], ["integration_github", "GitHub"], ["integration_vercel", "Vercel"],
  ["integration_n8n", "n8n"], ["integration_resend", "Resend"], ["integration_notion", "Notion"], ["integration_calcom", "Cal.com"],
];

/**
 * Service cards on the phone, read-only, one tick per service and none ticked until the user does it: what each card
 * says is a headline and three short lines, never an address, a link or a secret. Rust re-reads the ticks for every
 * connection and sends a phone only the services ticked here.
 */
function servicesBlock(): HTMLElement {
  const note = h("div", { class: "hint" });
  let ticked: string[] = [];
  const boxes = PHONE_SERVICES.map(([id, name]) => {
    const box = h("input", { type: "checkbox" }) as HTMLInputElement;
    const itemLabel = h("label", { class: "phone-service-chip" }, box, h("span", { text: name }));
    box.addEventListener("change", () => {
      itemLabel.classList.toggle("checked", box.checked);
      void (async () => {
        note.textContent = "";
        const next = box.checked ? [...new Set([...ticked, id])] : ticked.filter((x) => x !== id);
        try {
          ticked = (await Bridge.phoneServicesSet(next)).services;
          itemLabel.classList.toggle("checked", ticked.includes(id));
        } catch (err) {
          box.checked = ticked.includes(id);
          itemLabel.classList.toggle("checked", box.checked);
          note.textContent = String(err);
        }
      })();
    });
    return { id, box, itemLabel };
  });
  void Bridge.phoneServicesStatus().then((status) => {
    if (!status) return;
    ticked = status.services;
    for (const b of boxes) {
      b.box.checked = ticked.includes(b.id);
      b.itemLabel.classList.toggle("checked", b.box.checked);
    }
  });
  return h(
    "div",
    { class: "phone-perm-item" },
    h("div", { class: "phone-subhead", text: t("Show these services on the phone") }),
    h("div", { class: "hint", text: t("Each tick lets your paired phone see that service's card: a headline and up to three short lines, read-only. Nothing is shown until you tick it. No addresses, subjects, links or keys are ever sent.") }),
    h("div", { class: "phone-service-grid" }, ...boxes.map((b) => b.itemLabel)),
    note,
  );
}

/**
 * The plan usage on the phone: off until the user turns it on. Only the percentages and reset times the pills show,
 * for the Claude and Codex plans. Rust re-reads this switch for every connection.
 */
function usageBlock(makeToggle: Toggle): HTMLElement {
  const note = h("div", { class: "hint" });
  const switchEl = makeToggle(false, (next) => {
    void (async () => {
      note.textContent = "";
      try {
        await Bridge.phoneUsageSetEnabled(next);
      } catch (err) {
        switchEl.classList.remove("on");
        switchEl.setAttribute("aria-pressed", "false");
        note.textContent = String(err);
      }
    })();
  });
  void Bridge.phoneUsageStatus().then((status) => {
    if (!status) return;
    switchEl.classList.toggle("on", status.enabled);
    switchEl.setAttribute("aria-pressed", String(status.enabled));
  });
  return h(
    "div",
    { class: "phone-perm-item" },
    h(
      "div",
      { class: "phone-perm-row" },
      h(
        "div",
        { class: "phone-perm-copy" },
        h("label", { text: t("Show my plan usage on the phone") }),
        h("div", { class: "hint", text: t("The phone shows how much of your Claude and Codex plans is used and when they reset: the same percentages as the pills here, nothing else.") }),
      ),
      switchEl,
    ),
    note,
  );
}

/**
 * Last week's recap on the phone: off until the user turns it on. Only counts (minutes, sessions, files, lines, commands, permissions) and the
 * name of the busiest agent; the project's folder name only reaches a phone that also shows session details. Rust re-reads this switch for every
 * connection.
 */
function recapBlock(makeToggle: Toggle): HTMLElement {
  const note = h("div", { class: "hint" });
  const switchEl = makeToggle(false, (next) => {
    void (async () => {
      note.textContent = "";
      try {
        await Bridge.phoneRecapSetEnabled(next);
      } catch (err) {
        switchEl.classList.remove("on");
        switchEl.setAttribute("aria-pressed", "false");
        note.textContent = String(err);
      }
    })();
  });
  void Bridge.phoneRecapStatus().then((status) => {
    if (!status) return;
    switchEl.classList.toggle("on", status.enabled);
    switchEl.setAttribute("aria-pressed", String(status.enabled));
  });
  return h(
    "div",
    { class: "phone-perm-item" },
    h(
      "div",
      { class: "phone-perm-row" },
      h(
        "div",
        { class: "phone-perm-copy" },
        h("label", { text: t("Show my weekly recap on the phone") }),
        h("div", { class: "hint", text: t("The phone shows last week's recap: time coding, sessions, files, lines, commands, permissions and the agent you used most. Counts only. The project's name is added only if you also show session details.") }),
      ),
      switchEl,
    ),
    note,
  );
}

/**
 * The files an agent changed, on the phone: off until the user turns it on. The list (names and counts) rides on the
 * session; the lines of a file are sent only when the phone asks for that file. Rust re-reads this switch for every connection.
 */
function diffsBlock(makeToggle: Toggle): HTMLElement {
  const note = h("div", { class: "hint" });
  const switchEl = makeToggle(false, (next) => {
    void (async () => {
      note.textContent = "";
      try {
        await Bridge.phoneDiffsSetEnabled(next);
      } catch (err) {
        switchEl.classList.remove("on");
        switchEl.setAttribute("aria-pressed", "false");
        note.textContent = String(err);
      }
    })();
  });
  void Bridge.phoneDiffsStatus().then((status) => {
    if (!status) return;
    switchEl.classList.toggle("on", status.enabled);
    switchEl.setAttribute("aria-pressed", String(status.enabled));
  });
  return h(
    "div",
    { class: "phone-perm-item" },
    h(
      "div",
      { class: "phone-perm-row" },
      h(
        "div",
        { class: "phone-perm-copy" },
        h("label", { text: t("Show the files an agent changed on the phone") }),
        h("div", { class: "hint", text: t("The phone lists the files each agent changed and, when you tap one, shows what changed in it (200 lines at most, long lines cut). File names and lines go only to your paired phone, over the pinned encrypted link, and only when it asks.") }),
      ),
      switchEl,
    ),
    note,
  );
}

/**
 * Chat from the phone: off until the user turns it on, and then only the models ticked here.
 * The phone gets text; the keys stay on this computer (src-tauri/src/phone_link/chat.rs).
 */
function chatBlock(makeToggle: Toggle, settings: Settings): HTMLElement {
  let allowed: string[] = [];
  const body = h("div", { class: "phone-chat-providers" });
  const note = h("div", { class: "hint" });

  const save = async (next: string[]) => {
    try {
      allowed = (await Bridge.phoneChatSetModels(next)).models;
    } catch (err) {
      note.textContent = String(err);
    }
    emptyNote.hidden = allowed.length > 0;
  };
  const emptyNote = h("div", { class: "hint", text: t("Nothing is allowed yet: the phone cannot chat until you tick a model.") });

  /** One provider: a button that asks it for its models (only now), then a tick box per model. */
  const providerRow = (p: ProviderDef): HTMLElement => {
    const list = h("div", { class: "phone-chat-models" });
    const button = h("button", { text: t("Choose models") });
    const head = h("div", { class: "phone-chat-prov-head" }, h("span", { class: "phone-chat-prov-name", text: p.name }), button);
    const show = (models: string[]) => {
      list.replaceChildren(
        ...[...new Set([...models, ...modelsOf(allowed, p.id)])].map((model) => {
          const id = modelId(p.id, model);
          const box = h("input", { type: "checkbox" }) as HTMLInputElement;
          box.checked = allowed.includes(id);
          box.addEventListener("change", () => void save(toggleModel(allowed, id, box.checked)));
          return h("label", { class: "phone-model-check" }, box, h("span", { text: model }));
        }),
      );
    };
    button.addEventListener("click", () => {
      list.replaceChildren(h("div", { class: "hint", text: t("Loading models…") }));
      Bridge.chatModels(p.id).then(
        (models) => show(models.map((m) => m.id)),
        (err) => list.replaceChildren(h("div", { class: "hint", text: String(err).replace(/^Error:\s*/, "") })),
      );
    });
    return h("div", { class: "phone-chat-prov" }, head, list);
  };

  const draw = async (status: PhoneChatStatus) => {
    allowed = status.models;
    body.replaceChildren();
    if (!status.enabled) return;
    const hasKey: Record<string, boolean> = {};
    for (const p of usableProvidersCandidates()) hasKey[p.id] = (await Bridge.keyReady(p.key!)) ?? false;
    const providers = usableProviders(settings, hasKey);
    body.append(h("div", { class: "hint", text: t("Models the phone may use") }));
    if (providers.length === 0) body.append(h("div", { class: "hint", text: t("No provider is ready: add an API key or connect a local model first.") }));
    for (const p of providers) body.append(providerRow(p));
    emptyNote.hidden = allowed.length > 0;
    body.append(emptyNote);
  };

  const switchEl = makeToggle(false, (next) => {
    void (async () => {
      note.textContent = "";
      try {
        await draw(await Bridge.phoneChatSetEnabled(next));
      } catch (err) {
        switchEl.classList.remove("on");
        switchEl.setAttribute("aria-pressed", "false");
        body.replaceChildren();
        note.textContent = String(err);
      }
    })();
  });
  // The switch shows what Rust says, not what the page remembers.
  void Bridge.phoneChatStatus().then((status) => {
    if (!status) return;
    switchEl.classList.toggle("on", status.enabled);
    switchEl.setAttribute("aria-pressed", String(status.enabled));
    void draw(status);
  });

  return h(
    "div",
    { class: "phone-perm-item" },
    h(
      "div",
      { class: "phone-perm-row" },
      h(
        "div",
        { class: "phone-perm-copy" },
        h("label", { text: t("Let the phone chat with my AI providers") }),
        h("div", { class: "hint", text: t("Chat uses the API keys saved on this computer. The phone never sees them, and each message may cost money.") }),
      ),
      switchEl,
    ),
    note,
    body,
  );
}

function usableProvidersCandidates(): ProviderDef[] {
  return PROVIDERS.filter((p) => p.key !== null);
}

/**
 * Said after the link is copied: the link is the pairing code itself, so whoever reads it (the clipboard, a chat, a
 * screenshot) can pair a phone until a new code is made. Hidden until the link has been copied.
 */
export function copyLinkHint(): HTMLElement {
  const hint = h("div", {
    class: "hint",
    text: t("The link you copied contains the pairing code: anyone who gets it can pair a phone with this computer. If it ends up somewhere it should not, press Pair again to make a new code; the old one stops working."),
  });
  hint.hidden = true;
  return hint;
}

function pairingRows(pairing: PhoneLinkPairing, again: () => void): HTMLElement[] {
  // The QR comes from Rust (qrcode crate) as an SVG made only of paths.
  const qr = h("div", { class: "qr phone-qr" });
  qr.innerHTML = pairing.qrSvg;

  const link = h("input", { type: "text", class: "phone-mono-input phone-pair-link", readonly: "true", value: pairing.link }) as HTMLInputElement;
  link.addEventListener("focus", () => link.select());

  const copy = h("button", { class: "primary", text: t("Copy link") });
  const copied = copyLinkHint();
  copy.addEventListener("click", () => {
    void navigator.clipboard?.writeText(pairing.link).then(() => {
      copied.hidden = false; // stays after the button text is restored
      copy.textContent = t("Copied");
      window.setTimeout(() => (copy.textContent = t("Copy link")), 1500);
    });
  });

  const repair = h("button", { text: t("Pair again") });
  repair.addEventListener("click", again);

  const controls = h(
    "div",
    { class: "phone-pairing-controls" },
    h("div", { class: "hint", text: t("In Coucou for Android, scan this code with the camera or paste the link.") }),
    h("div", { class: "phone-input-row" }, link, copy),
    copied,
    ...(pairing.relay ? [h("div", { class: "hint", text: t("This code also carries the access key and the end-to-end key for the relay {host}. Pair again makes a new key and a new room.", { host: pairing.relay }) })] : []),
    h("div", { class: "phone-repair-row" }, repair, h("span", { class: "hint", text: t("The phone that was paired is disconnected and needs the new code.") })),
  );

  return [h("div", { class: "phone-pairing-sheet" }, qr, controls)];
}

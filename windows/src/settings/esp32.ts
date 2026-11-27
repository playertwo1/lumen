// ESP32-S3 AMOLED settings UI & validation functions (L9)

import { Bridge, type Esp32BridgeStatus } from "../core/bridge";
import { h } from "../views/dom";

type Toggle = (on: boolean, onChange: (v: boolean) => void) => HTMLElement;

// 1. Pure validation functions
export function isValidPairCode(code: string): boolean {
  return /^\d{6}$/.test(code);
}

export function isValidBridgePort(port: number): boolean {
  return Number.isInteger(port) && port >= 1024 && port <= 65535;
}

export function isValidRoomId(room: string): boolean {
  return /^[A-Za-z0-9_-]{8,64}$/.test(room);
}

export function isValidRelayUrl(url: string): boolean {
  if (!url || url.trim() === "") return false;
  return /^https:\/\/.+/.test(url) || /^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?(\/.*)?$/.test(url);
}

// 2. Section UI builder
export function esp32Section(makeToggle: Toggle): HTMLElement {
  const statusDot = h("span", { class: "phone-live-pill" });
  const modeBadge = h("span", { class: "phone-live-pill", text: "Desligada" });
  const note = h("div", { class: "notice err", hidden: true });

  const section = h(
    "section",
    { "data-anchor": "integrations.esp32", class: "phone-card" },
    h(
      "div",
      { class: "phone-card-head" },
      h(
        "div",
        { class: "phone-card-title-col" },
        h("div", { style: "display: flex; align-items: center; gap: 8px;" },
          h("h2", { text: "Placa ESP32-S3 AMOLED" }),
          statusDot,
          modeBadge,
        ),
        h("div", { class: "hint", text: "Gerencie a conexão da placa ESP32-S3 com o Lumen." }),
      ),
    ),
    note,
  );

  const showError = (msg: string) => {
    note.hidden = false;
    note.textContent = msg;
  };

  const clearError = () => {
    note.hidden = true;
    note.textContent = "";
  };

  // Bridge switch & port
  const portInput = h("input", { type: "number", value: "8787", class: "path" }) as HTMLInputElement;
  const localUrlDiv = h("div", { class: "hint", text: "URL Local: (carregando...)" });

  const updateUI = (st: Esp32BridgeStatus | null) => {
    if (!st) return;

    portInput.value = String(st.port);
    const running = st.running;
    statusDot.className = running ? "phone-live-pill ok" : "phone-live-pill";
    statusDot.textContent = running ? "Online" : "Offline";
    switchContainer.classList.toggle("on", st.enabled);
    switchContainer.setAttribute("aria-pressed", String(st.enabled));
    if (st.lastError) {
      showError(st.lastError);
    }

    let modeText = "Desligada";
    let modeOk = false;
    if (st.enabled) {
      if (st.mode === "lan") {
        modeText = "LAN";
        modeOk = true;
      } else if (st.mode === "relay") {
        modeText = "Ponte remota";
        modeOk = true;
      } else if (st.mode === "alone") {
        modeText = "Sozinha";
      }
    }
    modeBadge.className = modeOk ? "phone-live-pill ok" : "phone-live-pill";
    modeBadge.textContent = modeText;

    if (st.localUrls && st.localUrls.length > 0) {
      localUrlDiv.textContent = `URL para a placa: ${st.localUrls[0]}`;
    } else {
      localUrlDiv.textContent = `URL para a placa: http://<ip>:${st.port}`;
    }
  };

  const switchContainer = makeToggle(false, async (val) => {
    clearError();
    const port = parseInt(portInput.value, 10) || 8787;
    if (!isValidBridgePort(port)) {
      showError("Porta inválida (deve ser entre 1024 e 65535).");
      return;
    }
    try {
      const st = await Bridge.esp32.setEnabled(val, port);
      updateUI(st);
    } catch (err) {
      showError(String(err));
    }
  });

  section.append(
    h("div", { class: "row" },
      h("span", { text: "Ativar Ponte ESP32" }),
      switchContainer,
    ),
    h("div", { class: "row" },
      h("label", { text: "Porta LAN" }),
      portInput,
    ),
    localUrlDiv,
  );

  // Pairing code block
  const pairCodeInput = h("input", { type: "text", inputmode: "numeric", maxLength: 6, placeholder: "123456", class: "path" }) as HTMLInputElement;
  const pairStatusHint = h("div", { class: "hint", text: "Nenhum código armado." });

  const armBtn = h("button", { class: "primary", text: "Armar código" }) as HTMLButtonElement;
  armBtn.addEventListener("click", async () => {
    clearError();
    const code = pairCodeInput.value.trim();
    if (!isValidPairCode(code)) {
      showError("O código de pareamento deve ter exatamente 6 dígitos.");
      return;
    }
    try {
      const st = await Bridge.esp32.armPairCode(code);
      updateUI(st);
      if (st?.pairCodeArmed) {
        pairStatusHint.textContent = `Código armado (expira em ${st.pairCodeExpiresInSecs ?? 60}s).`;
      }
    } catch (err) {
      showError(String(err));
    }
  });

  const revokeBtn = h("button", { class: "danger", text: "Revogar placa" }) as HTMLButtonElement;
  revokeBtn.addEventListener("click", async () => {
    clearError();
    if (!window.confirm("Tem certeza que deseja revogar todas as placas pareadas?")) return;
    try {
      const st = await Bridge.esp32.revokeAll();
      updateUI(st);
      pairStatusHint.textContent = "Todas as placas foram revogadas.";
    } catch (err) {
      showError(String(err));
    }
  });

  section.append(
    h("div", { class: "phone-divider" }),
    h("h3", { text: "Pareamento Seguro" }),
    h("div", { class: "row" },
      pairCodeInput,
      armBtn,
    ),
    pairStatusHint,
    h("div", { class: "row" },
      revokeBtn,
    ),
  );

  // Relay Cloudflare block
  const relaySwitchInput = h("input", { type: "checkbox" }) as HTMLInputElement;
  const relayUrlInput = h("input", { type: "text", placeholder: "https://...", class: "path" }) as HTMLInputElement;
  const relayRoomInput = h("input", { type: "text", placeholder: "sala-exemplo", class: "path" }) as HTMLInputElement;
  const relayKeyInput = h("input", { type: "password", placeholder: "Já configurada — deixe vazio para manter", class: "path" }) as HTMLInputElement;

  const saveRelayBtn = h("button", { class: "primary", text: "Salvar relay" }) as HTMLButtonElement;
  let roomAlreadyConfigured = false;
  saveRelayBtn.addEventListener("click", async () => {
    clearError();
    const enabled = relaySwitchInput.checked;
    const url = relayUrlInput.value.trim();
    const room = relayRoomInput.value.trim();
    const key = relayKeyInput.value;

    if (enabled) {
      if (!isValidRelayUrl(url)) {
        showError("URL do Relay inválida.");
        return;
      }
      if ((room !== "" || !roomAlreadyConfigured) && !isValidRoomId(room)) {
        showError("Sala do Relay inválida (8 a 64 caracteres alfanuméricos, _ ou -).");
        return;
      }
    }

    try {
      const st = await Bridge.esp32.saveRelay(enabled, url, room, key);
      updateUI(st);
      if (st) {
        roomAlreadyConfigured = st.relayRoomConfigured;
        relayUrlInput.value = st.relayUrl || "";
        relayRoomInput.value = st.relayRoom || "";
        relayKeyInput.value = "";
        relayKeyInput.placeholder = st.relayKeyConfigured ? "Já configurada — deixe vazio para manter" : "";
      }
    } catch (err) {
      showError(String(err));
    }
  });

  section.append(
    h("div", { class: "phone-divider" }),
    h("h3", { text: "Relay Cloudflare" }),
    h("div", { class: "row" },
      h("span", { text: "Habilitar Relay" }),
      relaySwitchInput,
    ),
    h("div", { class: "row" },
      h("label", { text: "URL do Relay" }),
      relayUrlInput,
    ),
    h("div", { class: "row" },
      h("label", { text: "Sala" }),
      relayRoomInput,
    ),
    h("div", { class: "row" },
      h("label", { text: "Chave Relay" }),
      relayKeyInput,
    ),
    h("div", { class: "row" },
      saveRelayBtn,
    ),
  );

  // Agent Panel block (L9.6)
  const agentPanelSwitch = h("input", { type: "checkbox" }) as HTMLInputElement;
  const usagePortInput = h("input", { type: "number", value: "8737", class: "path" }) as HTMLInputElement;
  const saveAgentPanelBtn = h("button", { class: "primary", text: "Salvar Painel de Cota" }) as HTMLButtonElement;

  saveAgentPanelBtn.addEventListener("click", async () => {
    clearError();
    const enabled = agentPanelSwitch.checked;
    const usagePort = parseInt(usagePortInput.value, 10) || 8737;
    if (!isValidBridgePort(usagePort)) {
      showError("Porta do painel inválida.");
      return;
    }
    try {
      const st = await Bridge.esp32.setAgentPanel(enabled, usagePort);
      updateUI(st);
    } catch (err) {
      showError(String(err));
    }
  });

  section.append(
    h("div", { class: "phone-divider" }),
    h("h3", { text: "Painel de Cota e Agentes (L9.6)" }),
    h("div", { class: "row" },
      h("span", { text: "Ativar Painel de Cota" }),
      agentPanelSwitch,
    ),
    h("div", { class: "row" },
      h("label", { text: "Porta do Painel" }),
      usagePortInput,
    ),
    h("div", { class: "hint", text: "As rotas /api/tokens e /api/agents respondem nas duas portas sem precisar do tokenserver.py." }),
    h("div", { class: "row" },
      saveAgentPanelBtn,
    ),
  );

  // Initial load status + refresh ao vivo enquanto a aba estiver aberta
  const refreshStatus = (initial: boolean) => {
    void Bridge.esp32.status().then((st) => {
      if (!st) return;
      updateUI(st);
      if (initial) {
        roomAlreadyConfigured = st.relayRoomConfigured;
        relaySwitchInput.checked = st.relayEnabled;
        relayUrlInput.value = st.relayUrl || "";
        relayRoomInput.value = st.relayRoom || "";
        if (st.relayKeyConfigured) {
          relayKeyInput.placeholder = "Já configurada — deixe vazio para manter";
        }
        agentPanelSwitch.checked = st.agentPanelEnabled;
        usagePortInput.value = String(st.usagePort || 8737);
      }
    });
  };
  refreshStatus(true);
  const timer = setInterval(() => {
    if (!section.isConnected) {
      clearInterval(timer);
      return;
    }
    refreshStatus(false);
  }, 4000);

  return section;
}

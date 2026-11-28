// ESP32-S3 AMOLED settings UI & validation functions (L10 / Q2)

import { Bridge, type Esp32BridgeStatus } from "../core/bridge";
import { h } from "../views/dom";

type Toggle = (on: boolean, onChange: (v: boolean) => void) => HTMLElement;

// 1. Pure validation functions
export function isValidBridgePort(port: number): boolean {
  return Number.isInteger(port) && port >= 1024 && port <= 65535;
}

export function isValidRelayUrl(url: string): boolean {
  if (!url || url.trim() === "") return false;
  return /^https:\/\/.+/.test(url) || /^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?(\/.*)?$/.test(url);
}

export function validateMqttPairCode(raw: string): { ok: true; host: string; room: string } | { ok: false; error: string } {
  if (!raw || typeof raw !== "string" || !raw.trim()) {
    return { ok: false, error: "Código vazio." };
  }
  let parsed: any;
  try {
    parsed = JSON.parse(raw.trim());
  } catch {
    return { ok: false, error: "JSON inválido." };
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return { ok: false, error: "Formato inválido." };
  }
  if (parsed.t !== "sm-mqtt") {
    return { ok: false, error: "Tipo inválido (esperado 'sm-mqtt')." };
  }
  if (parsed.v !== 1) {
    return { ok: false, error: "Versão inválida (esperado 1)." };
  }
  if (typeof parsed.uri !== "string" || !parsed.uri.startsWith("mqtts://")) {
    return { ok: false, error: "URI deve começar com 'mqtts://'." };
  }
  let host = "";
  try {
    const u = new URL(parsed.uri);
    host = u.hostname;
    if (!host) {
      return { ok: false, error: "Host do broker inválido." };
    }
  } catch {
    return { ok: false, error: "URI do broker inválida." };
  }
  if (typeof parsed.u !== "string" || parsed.u.trim() === "") {
    return { ok: false, error: "Usuário não pode ser vazio." };
  }
  if (typeof parsed.p !== "string" || parsed.p.trim() === "") {
    return { ok: false, error: "Senha não pode ser vazia." };
  }
  if (typeof parsed.sala !== "string" || !/^[a-z0-9]{12}$/.test(parsed.sala)) {
    return { ok: false, error: "Sala deve conter exatamente 12 caracteres (a-z, 0-9)." };
  }
  if (typeof parsed.k !== "string" || !/^[0-9a-fA-F]{64}$/.test(parsed.k)) {
    return { ok: false, error: "Chave deve conter exatamente 64 caracteres hexadecimais." };
  }
  return { ok: true, host, room: parsed.sala };
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

  // MQTT block controls & displays
  const mqttCodeInput = h("textarea", {
    rows: "3",
    placeholder: 'Cole o código JSON exibido na tela da placa...',
    class: "path",
  }) as HTMLTextAreaElement;
  const connectMqttBtn = h("button", { class: "primary", text: "Conectar placa" }) as HTMLButtonElement;
  const removeMqttBtn = h("button", { class: "danger", text: "Esquecer placa" }) as HTMLButtonElement;

  const mqttHostSpan = h("span", { text: "—" });
  const mqttRoomSpan = h("span", { text: "—" });
  const mqttStatusSpan = h("span", { class: "phone-live-pill", text: "Não configurado" });
  const boardStatusSpan = h("span", { class: "phone-live-pill", text: "Placa offline" });

  // Relay Cloudflare block
  const relaySwitchInput = h("input", { type: "checkbox" }) as HTMLInputElement;
  const relayUrlInput = h("input", { type: "text", placeholder: "https://...", class: "path" }) as HTMLInputElement;
  const relayKeyInput = h("input", { type: "password", placeholder: "Já configurada — deixe vazio para manter", class: "path" }) as HTMLInputElement;
  const saveRelayBtn = h("button", { class: "primary", text: "Salvar relay" }) as HTMLButtonElement;

  // Agent Panel block (L9.6)
  const agentPanelSwitch = h("input", { type: "checkbox" }) as HTMLInputElement;
  const usagePortInput = h("input", { type: "number", value: "8737", class: "path" }) as HTMLInputElement;
  const saveAgentPanelBtn = h("button", { class: "primary", text: "Salvar Painel de Cota" }) as HTMLButtonElement;

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
      } else if (st.mode === "mqtt") {
        modeText = "MQTT";
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

    // MQTT display
    if (st.mqttConfigured) {
      mqttHostSpan.textContent = st.mqttHost || "—";
      mqttRoomSpan.textContent = st.mqttRoom || "—";
      if (st.mqttError) {
        mqttStatusSpan.textContent =
          st.mqttError === "Chave da placa mudou — cole o código novo"
            ? st.mqttError
            : `Erro: ${st.mqttError}`;
        mqttStatusSpan.className = "phone-live-pill err";
      } else if (st.mqttConnected) {
        mqttStatusSpan.textContent = "Conectado";
        mqttStatusSpan.className = "phone-live-pill ok";
      } else {
        mqttStatusSpan.textContent = "Conectando...";
        mqttStatusSpan.className = "phone-live-pill";
      }
      boardStatusSpan.textContent = st.mqttBoardOnline ? "Placa online" : "Placa offline";
      boardStatusSpan.className = st.mqttBoardOnline ? "phone-live-pill ok" : "phone-live-pill";
      removeMqttBtn.disabled = false;
    } else {
      mqttHostSpan.textContent = "—";
      mqttRoomSpan.textContent = "—";
      mqttStatusSpan.textContent = "Não configurado";
      mqttStatusSpan.className = "phone-live-pill";
      boardStatusSpan.textContent = "Placa offline";
      boardStatusSpan.className = "phone-live-pill";
      removeMqttBtn.disabled = true;
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

  // MQTT block event handlers
  connectMqttBtn.addEventListener("click", async () => {
    clearError();
    const raw = mqttCodeInput.value.trim();
    if (!raw) {
      showError("Cole o código da placa.");
      return;
    }
    const val = validateMqttPairCode(raw);
    if (!val.ok) {
      showError(val.error);
      return;
    }
    try {
      const st = await Bridge.esp32.saveMqttCode(raw);
      mqttCodeInput.value = ""; // Limpa o campo após salvar para não deixar credenciais visíveis na tela!
      updateUI(st);
    } catch (err) {
      showError(String(err));
    }
  });

  removeMqttBtn.addEventListener("click", async () => {
    clearError();
    if (!window.confirm("Tem certeza que deseja remover a conexão com a placa?")) return;
    try {
      const st = await Bridge.esp32.clearMqtt();
      updateUI(st);
    } catch (err) {
      showError(String(err));
    }
  });

  section.append(
    h("div", { class: "phone-divider" }),
    h("h3", { text: "Conexão MQTT (Código da placa)" }),
    h("div", { class: "hint", text: "Cole o código exibido na tela da placa para sincronizar tarefas e controlar mídia." }),
    h("div", { class: "row" },
      mqttCodeInput,
    ),
    h("div", { class: "row" },
      connectMqttBtn,
      removeMqttBtn,
    ),
    h("div", { class: "row" },
      h("label", { text: "Host do Broker" }),
      mqttHostSpan,
    ),
    h("div", { class: "row" },
      h("label", { text: "Sala" }),
      mqttRoomSpan,
    ),
    h("div", { class: "row" },
      h("label", { text: "Conexão MQTT" }),
      mqttStatusSpan,
    ),
    h("div", { class: "row" },
      h("label", { text: "Estado da Placa" }),
      boardStatusSpan,
    ),
  );

  // Relay Cloudflare event handler
  saveRelayBtn.addEventListener("click", async () => {
    clearError();
    const enabled = relaySwitchInput.checked;
    const url = relayUrlInput.value.trim();
    const key = relayKeyInput.value;

    if (enabled) {
      if (!isValidRelayUrl(url)) {
        showError("URL do Relay inválida.");
        return;
      }
    }

    try {
      const st = await Bridge.esp32.saveRelay(enabled, url, key);
      updateUI(st);
      if (st) {
        relayUrlInput.value = st.relayUrl || "";
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
      h("label", { text: "Chave Relay" }),
      relayKeyInput,
    ),
    h("div", { class: "row" },
      saveRelayBtn,
    ),
  );

  // Agent Panel event handler
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
        relaySwitchInput.checked = st.relayEnabled;
        relayUrlInput.value = st.relayUrl || "";
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

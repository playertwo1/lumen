// Login ChatGPT nas configurações do Chat: permite usar a assinatura do plano
// pelo navegador, alternar entre contas salvas no cofre do sistema e encerrar sessão.

import { Bridge, type ChatGptAccountStatus } from "../core/bridge";
import type { Settings } from "../core/state";
import { clear, h } from "../views/dom";

export interface ChatGptSectionDeps {
  settings: Settings;
  save: () => Promise<void>;
  onUpdated?: () => void;
  registerView?: (draw: () => void) => void;
}

export function chatGptAccountSection(
  initial: ChatGptAccountStatus | null,
  initialError: string | null,
  deps: ChatGptSectionDeps,
): HTMLElement {
  const { settings, save, onUpdated, registerView } = deps;
  const add = h("button", { class: "primary chatgpt-signin-btn", text: "Continuar com ChatGPT" }) as HTMLButtonElement;
  const body = h("div", { class: "chatgpt-body" });
  const section = h(
    "section",
    { class: "chatgpt-section" },
    h(
      "div",
      { class: "chatgpt-head" },
      h(
        "div",
        { class: "chatgpt-head-copy" },
        h("h2", {}, h("span", { text: "Usar seu plano ChatGPT" })),
        h("div", {
          class: "hint",
          text: "Conecte sua conta ChatGPT para conversar no chat usando o seu plano. As credenciais ficam no cofre do sistema.",
        }),
      ),
      add,
    ),
    body,
  );
  let status = initial;
  let error = initialError;
  let notice = "";
  let busy = false;

  add.addEventListener("click", () => void signIn());

  const draw = () => {
    clear(body);
    add.disabled = busy;
    add.textContent = busy ? "Aguardando o navegador…" : "Continuar com ChatGPT";
    if (error) body.append(h("div", { class: "notice err", text: error }));
    if (notice) body.append(h("div", { class: "notice ok", text: notice }));
    const accounts = status?.accounts ?? [];
    if (accounts.length > 0) {
      const list = h("div", { class: "chatgpt-accounts" });
      for (const account of accounts) {
        const state = !account.connected
          ? "Sessão desconectada"
          : account.planEnabled
            ? account.active ? "Conta ativa no chat" : "Autorização do plano confirmada"
            : "Conectada, sem autorização para usar o plano";
        const dotClass = !account.connected
          ? "chatgpt-dot off"
          : account.planEnabled
            ? account.active && settings.chatProvider === "chatgpt"
              ? "chatgpt-dot ok"
              : "chatgpt-dot ready"
            : "chatgpt-dot warn";
        const actions = h("div", { class: "chatgpt-account-actions" });
        if (account.connected && account.planEnabled && account.active) {
          const inUse = settings.chatProvider === "chatgpt";
          const useInChat = h("button", {
            class: inUse ? "chatgpt-inuse-btn" : "primary",
            text: inUse ? "Em uso" : "Usar no chat",
          }) as HTMLButtonElement;
          useInChat.disabled = busy || inUse;
          useInChat.addEventListener("click", () => void run(async () => {
            settings.chatProvider = "chatgpt";
            await save();
            onUpdated?.();
          }));
          actions.append(useInChat);
        }
        if (account.connected && account.planEnabled && !account.active) {
          const use = h("button", { text: "Usar esta conta" }) as HTMLButtonElement;
          use.disabled = busy;
          use.addEventListener("click", () => void run(async () => { status = await Bridge.chatGptActivate(account.profileId); }));
          actions.append(use);
        }
        const reconnect = h("button", { text: account.connected ? "Reautorizar" : "Conectar novamente" }) as HTMLButtonElement;
        reconnect.disabled = busy;
        reconnect.addEventListener("click", () => void signIn(account.profileId));
        actions.append(reconnect);
        if (account.connected) {
          const signOut = h("button", { class: "danger", text: "Sair" }) as HTMLButtonElement;
          signOut.disabled = busy;
          signOut.addEventListener("click", () => void endSession(account.profileId));
          actions.append(signOut);
        }
        list.append(
          h(
            "div",
            { class: "chatgpt-account-row" },
            h(
              "div",
              { class: "chatgpt-account-meta" },
              h("span", { class: dotClass, "aria-hidden": "true" }),
              h(
                "div",
                { class: "chatgpt-account-text" },
                h("strong", { text: account.label }),
                h("span", { class: "hint", text: state }),
              ),
            ),
            actions,
          ),
        );
      }
      body.append(list);
    }
  };

  async function run(action: () => Promise<void>) {
    busy = true;
    error = null;
    notice = "";
    draw();
    try {
      await action();
      status = await Bridge.chatGptStatus();
    } catch (err) {
      error = String(err).replace(/^Error:\s*/, "");
    } finally {
      busy = false;
      draw();
    }
  }

  async function signIn(profileId?: string) {
    busy = true;
    error = null;
    notice = "Conclua o login na janela do navegador. O Lumen não recebe sua senha.";
    draw();
    try {
      status = await Bridge.chatGptSignIn(profileId);
      notice = "Conta conectada e salva no cofre do sistema.";
    } catch (err) {
      error = String(err).replace(/^Error:\s*/, "");
      notice = "";
    } finally {
      busy = false;
      draw();
    }
  }

  async function endSession(profileId: string) {
    busy = true;
    error = null;
    notice = "";
    draw();
    try {
      const result = await Bridge.chatGptSignOut(profileId);
      status = await Bridge.chatGptStatus();
      const anyActive = Boolean(status?.accounts?.some((a) => a.connected && a.planEnabled));
      if (!anyActive && settings.chatProvider === "chatgpt") {
        settings.chatProvider = "anthropic";
        await save();
      }
      notice = result.localCleared
        ? result.remoteRevoked
          ? "Sessão encerrada e revogada."
          : "Tokens removidos deste computador; a revogação remota não foi confirmada. Você pode desconectar o Lumen nas configurações da sua conta ChatGPT."
        : "Não foi possível confirmar a limpeza local das credenciais. Confira o estado da conta antes de continuar.";
    } catch (err) {
      error = String(err).replace(/^Error:\s*/, "");
    } finally {
      busy = false;
      draw();
    }
  }

  registerView?.(draw);
  draw();
  return section;
}

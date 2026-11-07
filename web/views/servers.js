// web/views/servers.js
// Speedtest provider, LibreSpeed server & network interface settings view

import { escapeHtml } from "../i18n.js";

/**
 * Renders the Servers & Provider configuration view.
 *
 * @param {object} state - Application state
 * @param {object} [opts]
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t] - Translator function
 * @returns {string} HTML string
 */
export function renderServers(state, { t = (k) => k } = {}) {
  const currentProvider = state?.config?.provider || "cloudflare";
  const currentServerId = state?.config?.libreServerId != null ? String(state.config.libreServerId) : "auto";
  const currentUrl = state?.config?.libreUrl || "";
  const currentIface = state?.config?.interface || "auto";

  const interfaces = Array.isArray(state?.servers?.interfaces) ? state.servers.interfaces : [];
  const libreServers = Array.isArray(state?.servers?.libreServers) ? state.servers.libreServers : [];
  const libreError = state?.servers?.libreError || null;

  const title = t("servers.title") || "Servidores";

  // Header
  const headerHtml = `<div class="view-header">
  <h1 class="view-title">${escapeHtml(title)}</h1>
</div>`;

  // 1. Provider selection card
  const isCf = currentProvider === "cloudflare";
  const isLibre = currentProvider === "librespeed";

  const providerCardHtml = `<div class="card">
  <div class="card-header">
    <h2 class="card-title">${escapeHtml(t("servers.provider") || "Provedor de teste")}</h2>
  </div>
  <div class="form-group">
    <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 12px;">
      <label class="form-check provider-card-option" style="padding: 14px; border: 1px solid ${isCf ? "var(--accent-dl)" : "var(--border)"}; border-radius: 6px; cursor: pointer; background-color: ${isCf ? "var(--nav-active)" : "transparent"}; display: flex; align-items: center; gap: 10px;">
        <input type="radio" name="provider" value="cloudflare" ${isCf ? "checked" : ""}>
        <div>
          <div style="font-weight: 600; color: var(--text);">${escapeHtml(t("servers.providerCloudflare") || "Cloudflare")}</div>
        </div>
      </label>
      <label class="form-check provider-card-option" style="padding: 14px; border: 1px solid ${isLibre ? "var(--accent-dl)" : "var(--border)"}; border-radius: 6px; cursor: pointer; background-color: ${isLibre ? "var(--nav-active)" : "transparent"}; display: flex; align-items: center; gap: 10px;">
        <input type="radio" name="provider" value="librespeed" ${isLibre ? "checked" : ""}>
        <div>
          <div style="font-weight: 600; color: var(--text);">${escapeHtml(t("servers.providerLibrespeed") || "LibreSpeed")}</div>
        </div>
      </label>
    </div>
  </div>
</div>`;

  // 2. LibreSpeed public server & custom URL section
  const libreErrorBanner = libreError
    ? `<div class="banner banner-warning" style="margin-bottom: 16px;">
        <span>${escapeHtml(libreError)}</span>
      </div>`
    : "";

  let libreOptionsHtml = `<option value="auto"${currentServerId === "auto" || !currentServerId ? " selected" : ""}>${escapeHtml(t("servers.librespeedAuto") || "Automático (mais próximo)")}</option>`;
  for (const s of libreServers) {
    const isSel = String(s.id) === currentServerId;
    libreOptionsHtml += `\n<option value="${escapeHtml(s.id)}"${isSel ? " selected" : ""}>${escapeHtml(s.id)} — ${escapeHtml(s.name)}</option>`;
  }

  const libreDisabledAttr = !isLibre ? " disabled" : "";
  const libreCardStyle = !isLibre ? "opacity: 0.6;" : "";

  const libreCardHtml = `<div class="card" id="librespeed-card" style="${libreCardStyle}">
  <div class="card-header">
    <h2 class="card-title">${escapeHtml(t("servers.providerLibrespeed") || "LibreSpeed")}</h2>
  </div>
  ${libreErrorBanner}
  <div class="form-group">
    <label class="form-label" for="libre-server-id">${escapeHtml(t("servers.librespeedServer") || "Servidor LibreSpeed")}</label>
    <select id="libre-server-id" name="libreServerId" class="form-control"${libreDisabledAttr}>
      ${libreOptionsHtml}
    </select>
  </div>
  <div class="form-group">
    <label class="form-label" for="libre-url">${escapeHtml(t("servers.customUrl") || "URL personalizada")}</label>
    <input id="libre-url" type="url" name="libreUrl" value="${escapeHtml(currentUrl)}" placeholder="https://..." class="form-control"${libreDisabledAttr}>
    <span class="form-help">${escapeHtml(t("servers.customUrlHelp") || "URL base de uma instância própria do LibreSpeed (opcional)")}</span>
  </div>
</div>`;

  // 3. Network interface selector card
  const ifaceInList = currentIface && currentIface !== "auto" && interfaces.some((i) => i.name === currentIface);
  let ifaceOptionsHtml = `<option value="auto"${currentIface === "auto" || !currentIface ? " selected" : ""}>${escapeHtml(t("servers.interfaceAuto") || "Automático (padrão do sistema)")}</option>`;
  for (const iface of interfaces) {
    const isSel = iface.name === currentIface;
    const addrs = Array.isArray(iface.addrs) ? iface.addrs : [];
    const label = addrs.length > 0 ? `${iface.name} (${addrs.join(", ")})` : iface.name;
    ifaceOptionsHtml += `\n<option value="${escapeHtml(iface.name)}"${isSel ? " selected" : ""}>${escapeHtml(label)}</option>`;
  }
  if (currentIface && currentIface !== "auto" && !ifaceInList) {
    const unavailText = t("servers.unavailable") || "indisponível";
    ifaceOptionsHtml += `\n<option value="${escapeHtml(currentIface)}" selected>${escapeHtml(currentIface)} (${escapeHtml(unavailText)})</option>`;
  }

  const interfaceCardHtml = `<div class="card">
  <div class="card-header">
    <h2 class="card-title">${escapeHtml(t("servers.networkInterface") || "Interface de rede")}</h2>
  </div>
  <div class="form-group">
    <select name="interface" class="form-control">
      ${ifaceOptionsHtml}
    </select>
  </div>
</div>`;

  // Submit button
  const submitBtnHtml = `<div style="display: flex; justify-content: flex-end; margin-top: 16px;">
  <button type="submit" class="btn btn-primary">${escapeHtml(t("common.save") || "Salvar alterações")}</button>
</div>`;

  return `${headerHtml}
<form data-form="servers">
  ${providerCardHtml}
  ${libreCardHtml}
  ${interfaceCardHtml}
  ${submitBtnHtml}
</form>`;
}

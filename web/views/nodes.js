// web/views/nodes.js
// Multi-node management, remote node status cards, and context switcher

import { escapeHtml } from "../i18n.js";
import { formatNumber, formatBytes, formatMetric } from "../format.js";

/**
 * Formats uptime seconds into a human-readable duration string.
 */
function formatUptimeDuration(uptimeSecs) {
  if (uptimeSecs == null || !Number.isFinite(uptimeSecs) || uptimeSecs < 0) {
    return "—";
  }

  const days = Math.floor(uptimeSecs / 86400);
  const hours = Math.floor((uptimeSecs % 86400) / 3600);
  const mins = Math.floor((uptimeSecs % 3600) / 60);

  const parts = [];
  if (days > 0) parts.push(`${days}d`);
  if (hours > 0 || days > 0) parts.push(`${hours}h`);
  parts.push(`${mins}m`);

  return parts.join(" ");
}

/**
 * Extracts node payload from FormData for POST/PUT /api/nodes.
 */
export function extractNodePayload(formData) {
  const name = (formData.get("name") || "").trim();
  const url = (formData.get("url") || "").trim();
  const rawPwd = formData.get("password");
  const clearPwd = formData.has("clearPassword");

  const payload = { name, url };
  if (clearPwd) {
    payload.password = null;
  } else if (rawPwd != null && String(rawPwd).trim() !== "") {
    payload.password = String(rawPwd).trim();
  }

  return payload;
}

/**
 * Renders the topbar node switcher dropdown when remote nodes exist or a remote node is active.
 */
export function renderNodeSwitcher(state, { t = (k) => k } = {}) {
  const nodesState = state?.nodes || {};
  const items = Array.isArray(nodesState.items) ? nodesState.items : [];
  const activeNodeId = nodesState.activeNodeId ?? null;

  if (items.length === 0 && activeNodeId == null) {
    return "";
  }

  const localLabel = t("nodes.localTitle") || "Servidor local";
  let optionsHtml = `<option value="local"${activeNodeId == null ? " selected" : ""}>${escapeHtml(localLabel)}</option>`;
  for (const node of items) {
    const isSel = String(node.id) === String(activeNodeId);
    optionsHtml += `\n<option value="${escapeHtml(node.id)}"${isSel ? " selected" : ""}>${escapeHtml(node.name)}</option>`;
  }

  return `<select name="activeNodeSelector" class="form-control" style="padding: 4px 8px; font-size: 12px; width: auto;" aria-label="${escapeHtml(t("nodes.switchNode") || "Alternar nó")}">
    ${optionsHtml}
  </select>`;
}

/**
 * Renders the Nodes view: local instance status card, remote nodes list, and add/edit node form.
 *
 * @param {object} state - Application state
 * @param {object} [opts]
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t] - Translator function
 * @returns {string} HTML string
 */
export function renderNodes(state, { t = (k) => k } = {}) {
  const locale = state?.locale || "pt-BR";
  const title = t("nodes.title") || "Nós";
  const version = state?.system?.version || "—";
  const tz = state?.system?.tz || "UTC";
  const uptime = formatUptimeDuration(state?.system?.uptimeSecs);
  const cpuPercent = state?.system?.cpuPercent != null
    ? `${formatNumber(state.system.cpuPercent, locale, 1)}%`
    : "—";
  const ramMb = formatBytes(state?.system?.rssBytes, locale);

  const nodesState = state?.nodes || {};
  const items = Array.isArray(nodesState.items) ? nodesState.items : [];
  const statuses = nodesState.statuses || {};
  const activeNodeId = nodesState.activeNodeId ?? null;
  const editing = nodesState.editing || null;

  const headerHtml = `<div class="view-header">
  <h1 class="view-title">${escapeHtml(title)}</h1>
</div>`;

  // 1. Local instance card
  const isLocalActive = activeNodeId == null;
  const localActionHtml = isLocalActive
    ? `<span class="badge badge-auto">${escapeHtml(t("nodes.activeContext") || "Ativo")}</span>`
    : `<button type="button" class="btn btn-secondary btn-sm" data-action="switch-node" data-id="local">${escapeHtml(t("nodes.useNode") || "Selecionar")}</button>`;

  const localNodeHtml = `<div class="card" style="${isLocalActive ? "border-color: var(--accent-dl);" : ""}">
  <div class="card-header">
    <div style="display: flex; align-items: center; gap: 8px;">
      <span class="status-indicator online"></span>
      <h2 class="card-title">${escapeHtml(t("nodes.localTitle") || "Servidor local")}</h2>
      <span class="badge badge-ok">${escapeHtml(t("common.online") || "online")}</span>
    </div>
    <div>
      ${localActionHtml}
    </div>
  </div>
  <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 16px; margin-top: 12px;">
    <div>
      <div class="form-label" style="color: var(--secondary);">${escapeHtml(t("nodes.version") || "Versão")}</div>
      <div style="font-family: var(--font-mono); font-size: 15px; font-weight: 600; margin-top: 4px;">${escapeHtml(version)}</div>
    </div>
    <div>
      <div class="form-label" style="color: var(--secondary);">${escapeHtml(t("nodes.timezone") || "Fuso horário")}</div>
      <div style="font-family: var(--font-mono); font-size: 15px; font-weight: 600; margin-top: 4px;">${escapeHtml(tz)}</div>
    </div>
    <div>
      <div class="form-label" style="color: var(--secondary);">${escapeHtml(t("nodes.uptime") || "Tempo de atividade")}</div>
      <div style="font-family: var(--font-mono); font-size: 15px; font-weight: 600; margin-top: 4px;">${escapeHtml(uptime)}</div>
    </div>
    <div>
      <div class="form-label" style="color: var(--secondary);">${escapeHtml(t("nodes.cpu") || "Uso de CPU")}</div>
      <div style="font-family: var(--font-mono); font-size: 15px; font-weight: 600; margin-top: 4px;">${escapeHtml(cpuPercent)}</div>
    </div>
    <div>
      <div class="form-label" style="color: var(--secondary);">${escapeHtml(t("nodes.ram") || "Memória RAM")}</div>
      <div style="font-family: var(--font-mono); font-size: 15px; font-weight: 600; margin-top: 4px;">${escapeHtml(ramMb)}</div>
    </div>
  </div>
</div>`;

  // 2. Remote nodes cards
  let remoteNodesHtml = "";
  if (items.length === 0) {
    remoteNodesHtml = `<div class="card empty-state">
      <div class="empty-state-title">${escapeHtml(t("nodes.seamTitle") || "Múltiplos nós")}</div>
      <div class="empty-state-desc">${escapeHtml(t("nodes.emptyDescription") || "Nenhum nó remoto registrado. Adicione uma instância abaixo para monitorá-la remotamente.")}</div>
    </div>`;
  } else {
    remoteNodesHtml = items
      .map((node) => {
        const st = statuses[node.id] || {};
        const isOnline = Boolean(st.online);
        const isNodeActive = String(activeNodeId) === String(node.id);
        const latest = st.latestTest || null;

        const statusBadge = isOnline
          ? `<span class="badge badge-ok">${escapeHtml(t("common.online") || "online")}</span>`
          : `<span class="badge badge-error">${escapeHtml(t("common.offline") || "offline")}</span>`;

        const switchBtn = isNodeActive
          ? `<span class="badge badge-auto">${escapeHtml(t("nodes.activeContext") || "Ativo")}</span>`
          : `<button type="button" class="btn btn-primary btn-sm" data-action="switch-node" data-id="${escapeHtml(node.id)}">${escapeHtml(t("nodes.useNode") || "Selecionar")}</button>`;

        const dlStr = latest?.download != null ? `${formatMetric(latest.download, locale, "download")} Mbps` : "—";
        const ulStr = latest?.upload != null ? `${formatMetric(latest.upload, locale, "upload")} Mbps` : "—";
        const pingStr = latest?.ping != null ? `${formatMetric(latest.ping, locale, "ping")} ms` : "—";
        const verStr = st.version || "—";

        const errorRow = st.error && !isOnline
          ? `<div class="banner banner-error" style="margin-top: 12px; padding: 6px 10px; font-size: 12px;">
              <span>${escapeHtml(st.error)}</span>
            </div>`
          : "";

        return `<div class="card" style="${isNodeActive ? "border-color: var(--accent-dl);" : ""}">
          <div class="card-header">
            <div style="display: flex; align-items: center; gap: 8px; flex-wrap: wrap;">
              <span class="status-indicator ${isOnline ? "online" : "offline"}"></span>
              <h2 class="card-title">${escapeHtml(node.name)}</h2>
              ${statusBadge}
              <span class="card-subtitle" style="font-family: var(--font-mono);">${escapeHtml(node.url)}</span>
            </div>
            <div style="display: flex; align-items: center; gap: 6px;">
              ${switchBtn}
              <button type="button" class="btn btn-secondary btn-sm" data-action="edit-node" data-id="${escapeHtml(node.id)}">${escapeHtml(t("common.edit") || "Editar")}</button>
              <button type="button" class="btn btn-danger" data-action="delete-node" data-id="${escapeHtml(node.id)}">${escapeHtml(t("common.delete") || "Excluir")}</button>
            </div>
          </div>
          <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 12px; margin-top: 8px;">
            <div>
              <div class="form-label" style="color: var(--secondary);">${escapeHtml(t("metric.download") || "Download")}</div>
              <div class="num-col" style="font-size: 14px; font-weight: 600; color: var(--accent-dl); margin-top: 2px;">${escapeHtml(dlStr)}</div>
            </div>
            <div>
              <div class="form-label" style="color: var(--secondary);">${escapeHtml(t("metric.upload") || "Upload")}</div>
              <div class="num-col" style="font-size: 14px; font-weight: 600; color: var(--accent-ul); margin-top: 2px;">${escapeHtml(ulStr)}</div>
            </div>
            <div>
              <div class="form-label" style="color: var(--secondary);">${escapeHtml(t("metric.ping") || "Ping")}</div>
              <div class="num-col" style="font-size: 14px; font-weight: 600; color: var(--accent-ping); margin-top: 2px;">${escapeHtml(pingStr)}</div>
            </div>
            <div>
              <div class="form-label" style="color: var(--secondary);">${escapeHtml(t("nodes.version") || "Versão")}</div>
              <div class="num-col" style="font-size: 14px; font-weight: 600; margin-top: 2px;">${escapeHtml(verStr)}</div>
            </div>
          </div>
          ${errorRow}
        </div>`;
      })
      .join("\n");
  }

  // 3. Add / Edit Node Form card
  const formTitle = editing
    ? (t("nodes.editTitle") || "Editar nó remoto")
    : (t("nodes.addTitle") || "Adicionar nó remoto");

  const cancelBtnHtml = editing
    ? `<button type="button" class="btn btn-secondary" data-action="cancel-edit-node">${escapeHtml(t("common.cancel") || "Cancelar")}</button>`
    : "";

  const formCardHtml = `<div class="card">
    <div class="card-header">
      <h2 class="card-title">${escapeHtml(formTitle)}</h2>
    </div>
    <form data-form="node" data-edit-id="${escapeHtml(editing?.id ?? "")}">
      <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 14px;">
        <div class="form-group">
          <label class="form-label" for="node-name">${escapeHtml(t("nodes.nodeName") || "Nome do nó")}</label>
          <input id="node-name" type="text" name="name" value="${escapeHtml(editing?.name || "")}" class="form-control" required placeholder="Filial São Paulo">
        </div>
        <div class="form-group">
          <label class="form-label" for="node-url">${escapeHtml(t("nodes.nodeUrl") || "URL da instância")}</label>
          <input id="node-url" type="url" name="url" value="${escapeHtml(editing?.url || "")}" class="form-control" required placeholder="http://192.168.1.50:8080">
        </div>
        <div class="form-group">
          <label class="form-label" for="node-password">${escapeHtml(t("nodes.nodePassword") || "Senha / Token (opcional)")}</label>
          <input id="node-password" type="password" name="password" value="" class="form-control" placeholder="${editing?.hasPassword ? "••••••••" : ""}">
        </div>
      </div>
      <div style="display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px;">
        ${cancelBtnHtml}
        <button type="submit" class="btn btn-primary">${escapeHtml(t("common.save") || "Salvar alterações")}</button>
      </div>
    </form>
  </div>`;

  return `${headerHtml}
${localNodeHtml}
${remoteNodesHtml}
${formCardHtml}`;
}

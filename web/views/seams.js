// web/views/seams.js
// Nodes (local instance status + multi-node seam) and Integrations (Plan 3 seam) views

import { escapeHtml } from "../i18n.js";
import { formatNumber, formatBytes } from "../format.js";

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
 * Renders the Nodes view: local instance status card + multi-node seam.
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

  const headerHtml = `<div class="view-header">
  <h1 class="view-title">${escapeHtml(title)}</h1>
</div>`;

  // Local instance card
  const localNodeHtml = `<div class="card">
  <div class="card-header">
    <div style="display: flex; align-items: center; gap: 8px;">
      <span class="status-indicator online"></span>
      <h2 class="card-title">${escapeHtml(t("nodes.localTitle") || "Servidor local")}</h2>
    </div>
    <span class="badge badge-ok">${escapeHtml(t("common.online") || "online")}</span>
  </div>
  <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 16px; margin-top: 12px;">
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

  // Multi-node seam card (Plan 3)
  const seamHtml = `<div class="card empty-state">
  <div style="margin-bottom: 12px;">
    <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="var(--secondary)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
      <circle cx="18" cy="5" r="3"></circle>
      <circle cx="6" cy="12" r="3"></circle>
      <circle cx="18" cy="19" r="3"></circle>
      <line x1="8.59" y1="13.51" x2="15.42" y2="17.49"></line>
      <line x1="15.41" y1="6.51" x2="8.59" y2="10.49"></line>
    </svg>
  </div>
  <div class="empty-state-title">${escapeHtml(t("nodes.seamTitle") || "Múltiplos nós")}</div>
  <div class="empty-state-desc">${escapeHtml(t("nodes.seamDescription") || "O gerenciamento de nós remotos e instâncias distribuídas estará disponível nas próximas versões.")}</div>
</div>`;

  return `${headerHtml}
${localNodeHtml}
${seamHtml}`;
}

/**
 * Renders the Integrations empty state view ready for Plan 3.
 *
 * @param {object} state - Application state
 * @param {object} [opts]
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t] - Translator function
 * @returns {string} HTML string
 */
export function renderIntegrations(state, { t = (k) => k } = {}) {
  const title = t("integrations.title") || "Integrações";

  const headerHtml = `<div class="view-header">
  <h1 class="view-title">${escapeHtml(title)}</h1>
</div>`;

  const emptyCardHtml = `<div class="card empty-state">
  <div style="margin-bottom: 12px;">
    <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="var(--secondary)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
      <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"></path>
    </svg>
  </div>
  <div class="empty-state-title">${escapeHtml(t("integrations.emptyTitle") || "Notificações e integrações")}</div>
  <div class="empty-state-desc">${escapeHtml(t("integrations.emptyDescription") || "Integrações com Discord, Telegram, Webhooks e Prometheus estarão disponíveis nas próximas versões.")}</div>
</div>`;

  return `${headerHtml}
${emptyCardHtml}`;
}

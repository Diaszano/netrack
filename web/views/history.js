// web/views/history.js
// Paginated history table with full metrics, duration, error details, delete action, and load-more pagination

import { escapeHtml } from "../i18n.js";
import {
  formatMetric,
  formatDateTime,
  formatNumber,
} from "../format.js";

/**
 * Renders the History view.
 *
 * @param {object} state - Application state
 * @param {object} [opts]
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t] - Translator function
 * @returns {string} HTML string
 */
export function renderHistory(state, { t = (k) => k } = {}) {
  const locale = state?.locale || "pt-BR";
  const tz = state?.system?.tz || "UTC";

  // 1. Header
  const title = t("history.title") || "Histórico";
  const headerHtml = `<div class="view-header">
  <h1 class="view-title">${escapeHtml(title)}</h1>
</div>`;

  // 2. Table Rows
  const rows = Array.isArray(state?.history?.rows) ? state.history.rows : [];
  let tableRowsHtml = "";

  if (rows.length === 0) {
    const emptyText = t("history.empty") || "Nenhum teste no histórico";
    tableRowsHtml = `<tr><td colspan="10" style="text-align: center; padding: 24px; color: var(--secondary);">${escapeHtml(emptyText)}</td></tr>`;
  } else {
    tableRowsHtml = rows
      .map((row) => {
        const timeMs = row.createdAt ?? row.t;
        const timeStr = formatDateTime(timeMs, locale, tz);

        // Kind badge: auto vs custom (manual)
        const isAuto = row.kind === "auto";
        const kindLabel = isAuto
          ? (t("history.typeAuto") || "Automático")
          : (t("history.typeManual") || "Manual");
        const kindBadgeClass = isAuto ? "badge badge-auto" : "badge badge-manual";
        const kindBadge = `<span class="${kindBadgeClass}">${escapeHtml(kindLabel)}</span>`;

        // Provider & Server
        const provider = row.provider ? escapeHtml(row.provider) : "—";
        const serverName = row.serverName ? escapeHtml(row.serverName) : "";
        const serverHost = row.serverHost ? escapeHtml(row.serverHost) : "";
        let serverSub = "";
        if (serverName && serverHost) {
          serverSub = `${serverName} (${serverHost})`;
        } else if (serverName) {
          serverSub = serverName;
        } else if (serverHost) {
          serverSub = serverHost;
        }
        const serverCell = `<div>${provider}</div>${serverSub ? `<div class="card-subtitle">${serverSub}</div>` : ""}`;

        // Metrics: Ping, Jitter, Download, Upload, Duration
        const isFailed = Boolean(row.error);
        const pingStr = isFailed || row.ping == null ? "—" : `${formatMetric(row.ping, locale, "ping")} ms`;
        const jitterStr = isFailed || row.jitter == null ? "—" : `${formatMetric(row.jitter, locale, "jitter")} ms`;
        const dlStr = isFailed || row.download == null ? "—" : `${formatMetric(row.download, locale, "download")} Mbps`;
        const ulStr = isFailed || row.upload == null ? "—" : `${formatMetric(row.upload, locale, "upload")} Mbps`;

        const durStr = row.duration != null && Number.isFinite(row.duration)
          ? `${formatNumber(row.duration, locale, Number.isInteger(row.duration) ? 0 : 1)} s`
          : "—";

        // Status badge + escaped error details
        let statusCell = "";
        if (isFailed) {
          const errEscaped = escapeHtml(row.error);
          const failLabel = escapeHtml(t("status.failed") || "falha");
          statusCell = `<span class="badge badge-falha" style="color: #FF8FB1;" title="${errEscaped}">${failLabel}</span> <span class="error-detail" title="${errEscaped}" style="font-size: 11px; color: var(--secondary); margin-left: 4px;">${errEscaped}</span>`;
        } else {
          const okLabel = escapeHtml(t("status.ok") || "ok");
          statusCell = `<span class="badge badge-ok" style="color: #3FD6C2;">${okLabel}</span>`;
        }

        // Delete action button
        const delTitle = escapeHtml(t("common.delete") || "Excluir");
        const deleteBtn = `<button type="button" class="btn btn-danger" data-action="delete-test" data-id="${escapeHtml(row.id)}" title="${delTitle}">${delTitle}</button>`;

        return `<tr>
  <td class="num-col">${escapeHtml(timeStr)}</td>
  <td>${kindBadge}</td>
  <td>${serverCell}</td>
  <td class="num-col">${escapeHtml(pingStr)}</td>
  <td class="num-col">${escapeHtml(jitterStr)}</td>
  <td class="num-col">${escapeHtml(dlStr)}</td>
  <td class="num-col">${escapeHtml(ulStr)}</td>
  <td class="num-col">${escapeHtml(durStr)}</td>
  <td>${statusCell}</td>
  <td>${deleteBtn}</td>
</tr>`;
      })
      .join("\n");
  }

  // 3. Pagination Footer
  let paginationFooter = "";
  if (state?.history?.hasMore) {
    const loading = Boolean(state.history.loading);
    const loadMoreText = escapeHtml(t("action.loadMore") || "Carregar mais");
    paginationFooter = `<div class="pagination-footer" style="padding: 14px 16px; text-align: center; border-top: 1px solid var(--border);">
  <button type="button" class="btn btn-secondary" data-action="load-more-history"${loading ? " disabled" : ""}>${loadMoreText}</button>
</div>`;
  }

  const tableCardHtml = `<div class="card">
  <div class="table-container">
    <table class="dense-table">
      <thead>
        <tr>
          <th>${escapeHtml(t("history.colDateTime") || "Data/Hora")}</th>
          <th>${escapeHtml(t("history.colType") || "Tipo")}</th>
          <th>${escapeHtml(t("history.colServer") || "Provedor & Servidor")}</th>
          <th>${escapeHtml(t("history.colPing") || "Ping")}</th>
          <th>${escapeHtml(t("history.colJitter") || "Jitter")}</th>
          <th>${escapeHtml(t("history.colDownload") || "Download")}</th>
          <th>${escapeHtml(t("history.colUpload") || "Upload")}</th>
          <th>${escapeHtml(t("history.colDuration") || "Duração")}</th>
          <th>${escapeHtml(t("history.colStatus") || "Status")}</th>
          <th>${escapeHtml(t("history.colActions") || "Ações")}</th>
        </tr>
      </thead>
      <tbody>
        ${tableRowsHtml}
      </tbody>
    </table>
  </div>
  ${paginationFooter}
</div>`;

  return `<div class="view-history">
  ${headerHtml}
  ${tableCardHtml}
</div>`;
}

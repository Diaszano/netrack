// web/views/test-row.js
// Shared rendering of the server cell and OK/fail badge used by history and overview rows

import { escapeHtml } from "../i18n.js";

/** Provider plus "name (host)" subtitle, escaped. */
export function renderServerCell(row) {
  const provider = row.provider ? escapeHtml(row.provider) : "—";
  const serverName = row.serverName ? escapeHtml(row.serverName) : "";
  const serverHost = row.serverHost ? escapeHtml(row.serverHost) : "";
  const serverSub = serverName && serverHost
    ? `${serverName} (${serverHost})`
    : serverName || serverHost;
  return `<div>${provider}</div>${serverSub ? `<div class="card-subtitle">${serverSub}</div>` : ""}`;
}

/** OK badge, or fail badge whose title carries the escaped error. */
export function renderStatusBadge(row, { t = (k) => k } = {}) {
  if (row.error) {
    const err = escapeHtml(row.error);
    return `<span class="badge badge-falha" title="${err}">${escapeHtml(t("status.failed") || "falha")}</span>`;
  }
  return `<span class="badge badge-ok">${escapeHtml(t("status.ok") || "ok")}</span>`;
}

// web/views/overview.js
// Dense overview dashboard: header with trigger, 3 gauges, speed & heatmap charts, recent tests table

import { escapeHtml } from "../i18n.js";
import {
  formatMetric,
  formatTime,
  formatDateTime,
} from "../format.js";
import {
  renderGaugeCard,
  renderSpeedChart,
  renderHeatmap,
} from "../charts.js";

/**
 * Renders the Overview view.
 *
 * @param {object} state - Application state
 * @param {object} [opts]
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t] - Translator function
 * @returns {string} HTML string
 */
export function renderOverview(state, { t = (k) => k } = {}) {
  const locale = state?.locale || "pt-BR";
  const tz = state?.system?.tz || "UTC";

  // 1. Header with title, pause/next-run status pill, and test trigger button
  const title = t("overview.title") || "Visão geral";
  let pillText = "";
  const isPaused = Boolean(state?.status?.paused ?? state?.config?.paused);
  if (isPaused) {
    pillText = t("overview.pausedNotice") || "Agendamento pausado";
  } else if (state?.status?.nextRun) {
    const nextTime = formatTime(state.status.nextRun, locale, tz);
    pillText = t("overview.nextRun", { time: nextTime });
  }
  const pillHtml = pillText ? `<span class="status-pill">${escapeHtml(pillText)}</span>` : "";

  const isRunning = Boolean(state?.status?.running);
  const btnText = isRunning
    ? (t("action.running") || "Testando...")
    : `▶ ${t("action.runTest") || "Testar agora"}`;
  const runBtnHtml = `<button type="button" class="btn btn-primary" data-action="run-test"${isRunning ? " disabled" : ""}>${escapeHtml(btnText)}</button>`;

  const headerHtml = `<div class="view-header">
  <div class="view-header-status">
    <h1 class="view-title">${escapeHtml(title)}</h1>
    ${pillHtml}
  </div>
  <div class="view-header-actions">
    ${runBtnHtml}
  </div>
</div>`;

  // 2. 3 Gauge Cards (Download, Upload, Ping)
  const recent = Array.isArray(state?.recentTests) ? state.recentTests : [];
  const latestOk = recent.find((r) => !r.error && (r.download != null || r.upload != null || r.ping != null))
    || recent.find((r) => !r.error)
    || null;

  const dlGauge = renderGaugeCard({
    metric: "download",
    value: latestOk ? latestOk.download : null,
    unit: t("unit.mbps") || "Mbps",
    color: "#8B7CFF",
    expected: state?.config?.expectedDownload,
    avg: state?.overviewStats?.download?.avg,
    min: state?.overviewStats?.download?.min,
    locale,
    t,
  });

  const ulGauge = renderGaugeCard({
    metric: "upload",
    value: latestOk ? latestOk.upload : null,
    unit: t("unit.mbps") || "Mbps",
    color: "#3FD6C2",
    expected: state?.config?.expectedUpload,
    avg: state?.overviewStats?.upload?.avg,
    min: state?.overviewStats?.upload?.min,
    locale,
    t,
  });

  const pingGauge = renderGaugeCard({
    metric: "ping",
    value: latestOk ? latestOk.ping : null,
    unit: t("unit.ms") || "ms",
    color: "#FF8FB1",
    expected: state?.config?.expectedPing,
    avg: state?.overviewStats?.ping?.avg,
    max: state?.overviewStats?.ping?.max,
    jitter: latestOk?.jitter != null ? latestOk.jitter : (state?.overviewStats?.jitter?.avg ?? null),
    locale,
    t,
  });

  const gaugesHtml = `<div class="gauge-grid">
  ${dlGauge}
  ${ulGauge}
  ${pingGauge}
</div>`;

  // 3. Middle grid: 7d speed chart + download heatmap
  const speedChartHtml = renderSpeedChart(state?.overviewStats?.series || [], {
    locale,
    tz,
    t,
  });
  const heatmapHtml = renderHeatmap(state?.overviewStats?.heatmap || [], {
    locale,
    t,
  });

  const chartsHtml = `<div class="charts-grid">
  <div class="card">
    <div class="card-header">
      <div>
        <div class="card-title">${escapeHtml(t("overview.speedTitle") || "Velocidade · 7 dias")}</div>
        <div class="card-subtitle">${escapeHtml(t("overview.speedSubtitle") || "Últimos testes executados")}</div>
      </div>
    </div>
    <div class="speed-chart-container">
      ${speedChartHtml}
    </div>
  </div>
  <div class="card">
    <div class="card-header">
      <div class="card-title">${escapeHtml(t("overview.heatmapTitle") || "Download por dia e hora")}</div>
    </div>
    <div class="heatmap-container">
      ${heatmapHtml}
    </div>
  </div>
</div>`;

  // 4. Bottom card: Recent tests table (up to 10 rows)
  const recentRows = recent.slice(0, 10);
  let tableRowsHtml = "";
  if (recentRows.length === 0) {
    const emptyText = t("overview.noRecentTests") || "Nenhum teste registrado ainda";
    tableRowsHtml = `<tr><td colspan="6" style="text-align: center; padding: 24px; color: var(--secondary);">${escapeHtml(emptyText)}</td></tr>`;
  } else {
    tableRowsHtml = recentRows.map((row) => {
      const timeMs = row.createdAt ?? row.created_at ?? row.t;
      const timeStr = formatDateTime(timeMs, locale, tz);

      const isFailed = Boolean(row.error);
      const dlStr = isFailed || row.download == null ? "—" : `${formatMetric(row.download, locale, "download")} Mbps`;
      const ulStr = isFailed || row.upload == null ? "—" : `${formatMetric(row.upload, locale, "upload")} Mbps`;
      const pingStr = isFailed || row.ping == null ? "—" : `${formatMetric(row.ping, locale, "ping")} ms`;

      // Provider & Server
      const provider = row.provider ? escapeHtml(row.provider) : "—";
      const serverName = (row.serverName ?? row.server_name) ? escapeHtml(row.serverName ?? row.server_name) : "";
      const serverHost = (row.serverHost ?? row.server_host) ? escapeHtml(row.serverHost ?? row.server_host) : "";
      let serverSub = "";
      if (serverName && serverHost) {
        serverSub = `${serverName} (${serverHost})`;
      } else if (serverName) {
        serverSub = serverName;
      } else if (serverHost) {
        serverSub = serverHost;
      }
      const serverCell = `<div>${provider}</div>${serverSub ? `<div class="card-subtitle">${serverSub}</div>` : ""}`;

      // Status badge: ok in #3FD6C2, falha in #FF8FB1 with escaped error title
      let badgeHtml = "";
      if (isFailed) {
        const errTitle = escapeHtml(row.error);
        const failedLabel = escapeHtml(t("status.failed") || "falha");
        badgeHtml = `<span class="badge badge-falha" style="color: #FF8FB1;" title="${errTitle}">${failedLabel}</span>`;
      } else {
        const okLabel = escapeHtml(t("status.ok") || "ok");
        badgeHtml = `<span class="badge badge-ok" style="color: #3FD6C2;">${okLabel}</span>`;
      }

      return `<tr>
  <td class="num-col">${escapeHtml(timeStr)}</td>
  <td class="num-col">${escapeHtml(dlStr)}</td>
  <td class="num-col">${escapeHtml(ulStr)}</td>
  <td class="num-col">${escapeHtml(pingStr)}</td>
  <td>${serverCell}</td>
  <td>${badgeHtml}</td>
</tr>`;
    }).join("\n");
  }

  const tableCardHtml = `<div class="card">
  <div class="card-header">
    <div class="card-title">${escapeHtml(t("overview.recentTitle") || "Testes recentes")}</div>
  </div>
  <div class="table-container">
    <table class="dense-table">
      <thead>
        <tr>
          <th>${escapeHtml(t("history.colDateTime") || "Data/Hora")}</th>
          <th>${escapeHtml(t("history.colDownload") || "Download")}</th>
          <th>${escapeHtml(t("history.colUpload") || "Upload")}</th>
          <th>${escapeHtml(t("history.colPing") || "Ping")}</th>
          <th>${escapeHtml(t("history.colServer") || "Provedor & Servidor")}</th>
          <th>${escapeHtml(t("history.colStatus") || "Status")}</th>
        </tr>
      </thead>
      <tbody>
        ${tableRowsHtml}
      </tbody>
    </table>
  </div>
</div>`;

  return `<div class="view-overview">
  ${headerHtml}
  ${gaugesHtml}
  ${chartsHtml}
  ${tableCardHtml}
</div>`;
}

// web/views/statistics.js
// Statistics dashboard: preset bar, KPI summary strip, 4 metric summaries, period speed chart, hourly bars, heatmap

import { escapeHtml } from "../i18n.js";
import { formatNumber, formatMetric } from "../format.js";
import {
  renderSpeedChart,
  renderHourlyChart,
  renderHeatmap,
} from "../charts.js";

/**
 * Renders the Statistics view.
 *
 * @param {object} state - Application state
 * @param {object} [opts]
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t] - Translator function
 * @returns {string} HTML string
 */
export function renderStatistics(state, { t = (k) => k } = {}) {
  const locale = state?.locale || "pt-BR";
  const tz = state?.system?.tz || "UTC";
  const stats = state?.statsData || state?.overviewStats || {};

  // 1. Header with title and period bar (presets + from/to date inputs)
  const title = t("statistics.title") || "Estatísticas";
  const activePreset = state?.statsRange?.preset || "7d";
  const presets = [
    { id: "24h", label: t("statistics.preset24h") || "24h" },
    { id: "7d", label: t("statistics.preset7d") || "7 dias" },
    { id: "30d", label: t("statistics.preset30d") || "30 dias" },
    { id: "365d", label: t("statistics.preset365d") || "365 dias" },
  ];

  const presetButtonsHtml = presets
    .map((p) => {
      const activeClass = activePreset === p.id ? " active" : "";
      return `<button type="button" class="btn btn-preset${activeClass}" data-action="stats-preset" data-preset="${p.id}">${escapeHtml(p.label)}</button>`;
    })
    .join("\n    ");

  const fromVal = state?.statsRange?.from ? escapeHtml(state.statsRange.from) : "";
  const toVal = state?.statsRange?.to ? escapeHtml(state.statsRange.to) : "";

  const headerHtml = `<div class="view-header">
  <h1 class="view-title">${escapeHtml(title)}</h1>
  <div class="stats-period-bar" style="display: flex; align-items: center; gap: 12px; flex-wrap: wrap;">
    <div class="btn-group" style="display: flex; gap: 6px;">
      ${presetButtonsHtml}
    </div>
    <div class="date-inputs" style="display: flex; align-items: center; gap: 8px;">
      <label class="form-label" style="margin: 0; white-space: nowrap;">${escapeHtml(t("statistics.from") || "De")}</label>
      <input type="date" name="from" class="form-control" value="${fromVal}">
      <label class="form-label" style="margin: 0; white-space: nowrap;">${escapeHtml(t("statistics.to") || "Até")}</label>
      <input type="date" name="to" class="form-control" value="${toVal}">
    </div>
  </div>
</div>`;

  // 2. KPI Summary Strip (total, failed, availability%)
  const totalVal = stats.total != null && Number.isFinite(stats.total) ? formatNumber(stats.total, locale) : "0";
  const failedVal = stats.failed != null && Number.isFinite(stats.failed) ? formatNumber(stats.failed, locale) : "0";
  let availVal = "—";
  if (stats.availability != null && Number.isFinite(stats.availability)) {
    availVal = `${formatNumber(stats.availability, locale, 1)}%`;
  }

  const kpiStripHtml = `<div class="stats-summary-strip">
  <div class="stat-kpi-card">
    <div class="stat-kpi-label">${escapeHtml(t("statistics.total") || "Total de testes")}</div>
    <div class="stat-kpi-value">${escapeHtml(totalVal)}</div>
  </div>
  <div class="stat-kpi-card">
    <div class="stat-kpi-label">${escapeHtml(t("statistics.failures") || "Falhas")}</div>
    <div class="stat-kpi-value">${escapeHtml(failedVal)}</div>
  </div>
  <div class="stat-kpi-card">
    <div class="stat-kpi-label">${escapeHtml(t("statistics.availability") || "Disponibilidade")}</div>
    <div class="stat-kpi-value">${escapeHtml(availVal)}</div>
  </div>
</div>`;

  // 3. 4 Metric Summaries (Download, Upload, Ping, Jitter)
  const metricConfigs = [
    { key: "download", labelKey: "metric.download", unit: t("unit.mbps") || "Mbps" },
    { key: "upload", labelKey: "metric.upload", unit: t("unit.mbps") || "Mbps" },
    { key: "ping", labelKey: "metric.ping", unit: t("unit.ms") || "ms" },
    { key: "jitter", labelKey: "metric.jitter", unit: t("unit.ms") || "ms" },
  ];

  const metricCardsHtml = metricConfigs
    .map(({ key, labelKey, unit }) => {
      const m = stats[key] || {};
      const metricTitle = t(labelKey) || key.charAt(0).toUpperCase() + key.slice(1);

      const avgVal = m.avg != null && Number.isFinite(m.avg) ? formatMetric(m.avg, locale, key) : "—";
      const minVal = m.min != null && Number.isFinite(m.min) ? formatMetric(m.min, locale, key) : "—";
      const maxVal = m.max != null && Number.isFinite(m.max) ? formatMetric(m.max, locale, key) : "—";

      const std = m.stdDev;
      const stdVal = std != null && Number.isFinite(std) ? formatMetric(std, locale, key) : "—";

      let consistencyVal = "—";
      if (m.consistency != null && Number.isFinite(m.consistency)) {
        consistencyVal = `${formatNumber(m.consistency, locale, 1)}%`;
      }

      return `<div class="card stat-metric-card" style="margin-bottom: 0;">
  <div class="card-header" style="margin-bottom: 10px;">
    <div class="card-title">${escapeHtml(metricTitle)}</div>
    <div class="card-subtitle">${escapeHtml(unit)}</div>
  </div>
  <div class="stat-metric-body" style="display: flex; flex-direction: column; gap: 6px; font-size: 12px;">
    <div class="stat-metric-row" style="display: flex; justify-content: space-between;">
      <span style="color: var(--secondary);">${escapeHtml(t("statistics.avg") || "Média")}</span>
      <span class="num-col">${escapeHtml(avgVal)}</span>
    </div>
    <div class="stat-metric-row" style="display: flex; justify-content: space-between;">
      <span style="color: var(--secondary);">${escapeHtml(t("statistics.min") || "Mínimo")}</span>
      <span class="num-col">${escapeHtml(minVal)}</span>
    </div>
    <div class="stat-metric-row" style="display: flex; justify-content: space-between;">
      <span style="color: var(--secondary);">${escapeHtml(t("statistics.max") || "Máximo")}</span>
      <span class="num-col">${escapeHtml(maxVal)}</span>
    </div>
    <div class="stat-metric-row" style="display: flex; justify-content: space-between;">
      <span style="color: var(--secondary);">${escapeHtml(t("statistics.stdDev") || "Desvio padrão")}</span>
      <span class="num-col">${escapeHtml(stdVal)}</span>
    </div>
    <div class="stat-metric-row" style="display: flex; justify-content: space-between;">
      <span style="color: var(--secondary);">${escapeHtml(t("statistics.consistency") || "Consistência")}</span>
      <span class="num-col">${escapeHtml(consistencyVal)}</span>
    </div>
  </div>
</div>`;
    })
    .join("\n");

  const metricsGridHtml = `<div class="gauge-grid" style="grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); margin-bottom: 16px;">
  ${metricCardsHtml}
</div>`;

  // 4. Charts Grid: Speed chart, Hourly bar chart, Heatmap
  const speedChartHtml = renderSpeedChart(stats.series || [], {
    locale,
    tz,
    t,
  });
  const hourlyChartHtml = renderHourlyChart(stats.hourly || [], {
    locale,
    t,
  });
  const heatmapHtml = renderHeatmap(stats.heatmap || [], {
    locale,
    t,
  });

  const chartsHtml = `<div class="charts-grid">
  <div class="card">
    <div class="card-header">
      <div class="card-title">${escapeHtml(t("statistics.speedChartTitle") || "Velocidade no período")}</div>
    </div>
    <div class="speed-chart-container">
      ${speedChartHtml}
    </div>
  </div>
  <div class="card">
    <div class="card-header">
      <div class="card-title">${escapeHtml(t("statistics.hourlyChartTitle") || "Média por hora do dia")}</div>
    </div>
    <div class="hourly-chart-container">
      ${hourlyChartHtml}
    </div>
  </div>
</div>
<div class="card" style="margin-top: 16px;">
  <div class="card-header">
    <div class="card-title">${escapeHtml(t("statistics.heatmapTitle") || "Mapa de calor de download")}</div>
  </div>
  <div class="heatmap-container">
    ${heatmapHtml}
  </div>
</div>`;

  // 5. Per provider/server comparison
  const fmt = (v, key) => (v != null && Number.isFinite(v) ? formatMetric(v, locale, key) : "—");
  const byServer = Array.isArray(stats.byServer) ? stats.byServer : [];
  const byServerHtml = byServer.length
    ? `<div class="card" style="margin-top: 16px;">
  <div class="card-header">
    <div class="card-title">${escapeHtml(t("statistics.byServerTitle") || "Por provedor e servidor")}</div>
  </div>
  <div class="table-container">
    <table class="dense-table">
      <thead>
        <tr>
          <th>${escapeHtml(t("history.colServer") || "Provedor & Servidor")}</th>
          <th>${escapeHtml(t("statistics.total") || "Total de testes")}</th>
          <th>${escapeHtml(t("statistics.failures") || "Falhas")}</th>
          <th>${escapeHtml(t("history.colDownload") || "Download")}</th>
          <th>${escapeHtml(t("history.colUpload") || "Upload")}</th>
          <th>${escapeHtml(t("history.colPing") || "Ping")}</th>
        </tr>
      </thead>
      <tbody>
        ${byServer
          .map(
            (s) => `<tr>
          <td>${escapeHtml(s.provider)} · ${escapeHtml(s.serverName || t("statistics.unknownServer") || "sem servidor")}</td>
          <td class="num-col">${escapeHtml(formatNumber(s.total, locale))}</td>
          <td class="num-col">${escapeHtml(formatNumber(s.failed, locale))}</td>
          <td class="num-col">${escapeHtml(fmt(s.download, "download"))}</td>
          <td class="num-col">${escapeHtml(fmt(s.upload, "upload"))}</td>
          <td class="num-col">${escapeHtml(fmt(s.ping, "ping"))}</td>
        </tr>`,
          )
          .join("")}
      </tbody>
    </table>
  </div>
</div>`
    : "";

  return `<div class="view-statistics">
  ${headerHtml}
  ${kpiStripHtml}
  ${metricsGridHtml}
  ${chartsHtml}
  ${byServerHtml}
</div>`;
}

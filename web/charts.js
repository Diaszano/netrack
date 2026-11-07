// web/charts.js
// Pure SVG gauges, speed area chart, weekday x hour heatmap, and hourly bar chart

import { escapeHtml } from "./i18n.js";
import { formatMetric, formatTime, computePlanProgress } from "./format.js";

/**
 * 240-degree circular arc gauge card matching Penpot "C · Painel denso".
 *
 * @param {object} props
 * @param {"download"|"upload"|"ping"} props.metric
 * @param {number|null|undefined} props.value
 * @param {string} props.unit - "Mbps" or "ms"
 * @param {string} props.color - Hex color string (e.g. "#8B7CFF", "#3FD6C2", "#FF8FB1")
 * @param {number|null|undefined} props.expected - Expected plan speed or ping target
 * @param {number|null|undefined} props.avg
 * @param {number|null|undefined} [props.min] - Min value (used for download / upload)
 * @param {number|null|undefined} [props.max] - Max value (used for ping)
 * @param {number|null|undefined} [props.jitter] - Jitter value (used for ping)
 * @param {string} props.locale
 * @param {(key: string, vars?: Record<string, string | number>) => any} [props.t]
 * @returns {string} HTML string
 */
export function renderGaugeCard({
  metric,
  value,
  unit,
  color,
  expected,
  avg,
  min,
  max,
  jitter,
  locale = "pt-BR",
  t,
}) {
  const safeMetric = metric || "download";
  const { percent, ratio } = computePlanProgress(value, expected, safeMetric);

  // 240-degree arc geometry: center (60, 52), radius 44.
  // Start: 150 deg (bottom-left) -> Sweep 240 deg clockwise -> End: 30 deg (bottom-right).
  // Arc length = (240 / 360) * 2 * PI * 44 = (4 * PI / 3) * 44 ≈ 184.307
  const totalLength = 184.307;
  const safeRatio = Number.isFinite(ratio) ? Math.max(0, Math.min(1, ratio)) : 0;
  const dash = (safeRatio * totalLength).toFixed(2);
  const trackPath = "M 21.895 74 A 44 44 0 1 1 98.105 74";

  const displayVal = value == null ? "—" : formatMetric(value, locale, safeMetric);

  // Title: "Download", "Upload", "Ping"
  const titleKey = `metric.${safeMetric}`;
  const title = t
    ? t(titleKey)
    : safeMetric.charAt(0).toUpperCase() + safeMetric.slice(1);

  // Subtitle:
  // For ping: "jitter 1,8"
  // For download/upload: "{pct}% do plano" (or "—" if no plan)
  let subtitle;
  if (safeMetric === "ping") {
    const jitterVal = formatMetric(jitter, locale, "jitter");
    subtitle = t
      ? t("overview.jitter", { val: jitterVal })
      : `jitter ${jitterVal}`;
  } else {
    if (percent != null) {
      subtitle = t
        ? t("overview.planPercent", { pct: percent })
        : `${percent}% do plano`;
    } else {
      subtitle = "—";
    }
  }

  // Average
  const avgVal = formatMetric(avg, locale, safeMetric);
  const avgText = t
    ? t("overview.avg", { val: avgVal })
    : `méd ${avgVal}`;

  // Secondary metric: min (download/upload) or max (ping)
  let secondaryText;
  if (safeMetric === "ping") {
    const maxVal = formatMetric(max, locale, safeMetric);
    secondaryText = t
      ? t("overview.max", { val: maxVal })
      : `máx ${maxVal}`;
  } else {
    const minVal = formatMetric(min, locale, safeMetric);
    secondaryText = t
      ? t("overview.min", { val: minVal })
      : `mín ${minVal}`;
  }

  const arcOpacity = safeRatio > 0 && value != null ? 'stroke-opacity="1"' : 'stroke-opacity="0"';

  return `<div class="card gauge-card">
  <svg class="gauge-svg" width="120" height="104" viewBox="0 0 120 104" role="img" aria-label="${escapeHtml(title)}: ${escapeHtml(displayVal)} ${escapeHtml(unit)}">
    <path class="gauge-track" d="${trackPath}" fill="none" stroke="#262A47" stroke-width="10" stroke-linecap="round" />
    <path class="gauge-value" d="${trackPath}" fill="none" stroke="${escapeHtml(color)}" stroke-width="10" stroke-linecap="round" stroke-dasharray="${dash} ${totalLength.toFixed(2)}" ${arcOpacity} />
    <text class="gauge-number" x="60" y="54" text-anchor="middle" dominant-baseline="central" font-family="'Space Grotesk', system-ui, sans-serif" font-size="32" font-weight="600" fill="#E8E9F5">${escapeHtml(displayVal)}</text>
    <text class="gauge-unit" x="60" y="74" text-anchor="middle" font-family="'Inter', system-ui, sans-serif" font-size="12" fill="#8A8DB0">${escapeHtml(unit)}</text>
  </svg>
  <div class="gauge-meta">
    <div class="gauge-title">${escapeHtml(title)}</div>
    <div class="gauge-subtitle">${escapeHtml(subtitle)}</div>
    <div class="gauge-stat gauge-avg">${escapeHtml(avgText)}</div>
    <div class="gauge-stat gauge-minmax">${escapeHtml(secondaryText)}</div>
  </div>
</div>`;
}

/**
 * 612x210 SVG speed area chart comparing download and upload over time.
 *
 * @param {Array<{ t: number|string|Date, download: number|null, upload: number|null, ping?: number|null }>} series
 * @param {object} opts
 * @param {string} opts.locale
 * @param {string} opts.tz - Server IANA timezone
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t]
 * @returns {string} SVG or empty-state HTML
 */
export function renderSpeedChart(series, { locale = "pt-BR", tz = "UTC", t } = {}) {
  if (!Array.isArray(series) || series.length === 0) {
    const emptyMsg = t ? t("overview.chartEmpty") : "Sem dados suficientes para exibir o gráfico";
    return `<div class="chart-empty">${escapeHtml(emptyMsg)}</div>`;
  }

  const width = 612;
  const height = 210;
  const plotTop = 30;
  const plotBottom = 165;
  const plotHeight = plotBottom - plotTop;

  // 4 horizontal grid lines
  const gridLines = [
    plotTop,
    plotTop + plotHeight * (1 / 3),
    plotTop + plotHeight * (2 / 3),
    plotBottom,
  ].map((y) => `<line x1="0" y1="${y.toFixed(1)}" x2="${width}" y2="${y.toFixed(1)}" stroke="#262A47" stroke-width="1" />`).join("\n    ");

  // Determine max value for Y scaling
  let maxSpeed = 1;
  for (const p of series) {
    if (Number.isFinite(p.download) && p.download > maxSpeed) {
      maxSpeed = p.download;
    }
    if (Number.isFinite(p.upload) && p.upload > maxSpeed) {
      maxSpeed = p.upload;
    }
  }
  const maxY = maxSpeed * 1.15;

  function toY(val) {
    if (val == null || !Number.isFinite(val) || val <= 0) {
      return plotBottom;
    }
    const ratio = Math.min(1, val / maxY);
    return plotBottom - ratio * plotHeight;
  }

  // Handle single-point series
  if (series.length === 1) {
    const p = series[0];
    const dlY = toY(p.download);
    const ulY = toY(p.upload);
    const timeLabel = formatTime(p.t, locale, tz);

    const dlAreaPath = `M 0,${dlY.toFixed(1)} L ${width},${dlY.toFixed(1)} L ${width},${plotBottom} L 0,${plotBottom} Z`;
    const dlLinePath = `M 0,${dlY.toFixed(1)} L ${width},${dlY.toFixed(1)}`;

    const ulAreaPath = `M 0,${ulY.toFixed(1)} L ${width},${ulY.toFixed(1)} L ${width},${plotBottom} L 0,${plotBottom} Z`;
    const ulLinePath = `M 0,${ulY.toFixed(1)} L ${width},${ulY.toFixed(1)}`;

    const midX = (width / 2).toFixed(1);
    const dlPoint = `<circle class="chart-point" cx="${midX}" cy="${dlY.toFixed(1)}" r="4" fill="#8B7CFF" data-t="${escapeHtml(p.t)}" data-dl="${escapeHtml(p.download)}" data-ul="${escapeHtml(p.upload)}" data-ping="${escapeHtml(p.ping ?? "")}" />`;
    const ulPoint = `<circle class="chart-point" cx="${midX}" cy="${ulY.toFixed(1)}" r="4" fill="#3FD6C2" data-t="${escapeHtml(p.t)}" data-dl="${escapeHtml(p.download)}" data-ul="${escapeHtml(p.upload)}" data-ping="${escapeHtml(p.ping ?? "")}" />`;

    return `<svg viewBox="0 0 ${width} ${height}" class="speed-chart" role="img" aria-label="Speed chart">
    ${gridLines}
    <path d="${dlAreaPath}" fill="#8B7CFF" fill-opacity="0.15" />
    <path d="${dlLinePath}" fill="none" stroke="#8B7CFF" stroke-width="2" />
    <path d="${ulAreaPath}" fill="#3FD6C2" fill-opacity="0.12" />
    <path d="${ulLinePath}" fill="none" stroke="#3FD6C2" stroke-width="2" />
    ${dlPoint}
    ${ulPoint}
    <text x="${midX}" y="195" text-anchor="middle" font-family="'JetBrains Mono', monospace" font-size="11" fill="#8A8DB0">${escapeHtml(timeLabel)}</text>
  </svg>`;
  }

  // Multi-point series
  const n = series.length;
  const padLeft = 20;
  const padRight = 20;
  const plotWidth = width - padLeft - padRight;

  const dlPoints = [];
  const ulPoints = [];
  const circles = [];

  for (let i = 0; i < n; i++) {
    const p = series[i];
    const x = padLeft + (i / (n - 1)) * plotWidth;
    const dlY = toY(p.download);
    const ulY = toY(p.upload);

    dlPoints.push({ x, y: dlY });
    ulPoints.push({ x, y: ulY });

    circles.push(
      `<circle class="chart-point" cx="${x.toFixed(1)}" cy="${dlY.toFixed(1)}" r="4" fill="#8B7CFF" data-t="${escapeHtml(p.t)}" data-dl="${escapeHtml(p.download)}" data-ul="${escapeHtml(p.upload)}" data-ping="${escapeHtml(p.ping ?? "")}" />`
    );
    circles.push(
      `<circle class="chart-point" cx="${x.toFixed(1)}" cy="${ulY.toFixed(1)}" r="4" fill="#3FD6C2" data-t="${escapeHtml(p.t)}" data-dl="${escapeHtml(p.download)}" data-ul="${escapeHtml(p.upload)}" data-ping="${escapeHtml(p.ping ?? "")}" />`
    );
  }

  const dlLinePath = `M ${dlPoints.map((pt) => `${pt.x.toFixed(1)},${pt.y.toFixed(1)}`).join(" L ")}`;
  const dlAreaPath = `M ${dlPoints[0].x.toFixed(1)},${plotBottom} L ${dlPoints.map((pt) => `${pt.x.toFixed(1)},${pt.y.toFixed(1)}`).join(" L ")} L ${dlPoints[n - 1].x.toFixed(1)},${plotBottom} Z`;

  const ulLinePath = `M ${ulPoints.map((pt) => `${pt.x.toFixed(1)},${pt.y.toFixed(1)}`).join(" L ")}`;
  const ulAreaPath = `M ${ulPoints[0].x.toFixed(1)},${plotBottom} L ${ulPoints.map((pt) => `${pt.x.toFixed(1)},${pt.y.toFixed(1)}`).join(" L ")} L ${ulPoints[n - 1].x.toFixed(1)},${plotBottom} Z`;

  // X-axis time labels: show 3 to 5 evenly distributed labels
  const labelIndices = [];
  if (n <= 5) {
    for (let i = 0; i < n; i++) labelIndices.push(i);
  } else {
    const step = (n - 1) / 4;
    for (let k = 0; k <= 4; k++) {
      labelIndices.push(Math.round(k * step));
    }
  }

  const timeLabels = labelIndices
    .map((idx) => {
      const p = series[idx];
      const x = padLeft + (idx / (n - 1)) * plotWidth;
      const formatted = formatTime(p.t, locale, tz);
      return `<text x="${x.toFixed(1)}" y="195" text-anchor="middle" font-family="'JetBrains Mono', monospace" font-size="11" fill="#8A8DB0">${escapeHtml(formatted)}</text>`;
    })
    .join("\n    ");

  return `<svg viewBox="0 0 ${width} ${height}" class="speed-chart" role="img" aria-label="Speed chart">
    ${gridLines}
    <path d="${dlAreaPath}" fill="#8B7CFF" fill-opacity="0.15" />
    <path d="${dlLinePath}" fill="none" stroke="#8B7CFF" stroke-width="2" />
    <path d="${ulAreaPath}" fill="#3FD6C2" fill-opacity="0.12" />
    <path d="${ulLinePath}" fill="none" stroke="#3FD6C2" stroke-width="2" />
    ${circles.join("\n    ")}
    ${timeLabels}
  </svg>`;
}

/**
 * 7 weekday rows x 24 hour cells heatmap.
 *
 * @param {Array<{ weekday: number, hour: number, download: number|null, count?: number }>} cells
 * @param {object} opts
 * @param {string} opts.locale
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t]
 * @returns {string} HTML string
 */
export function renderHeatmap(cells = [], { locale = "pt-BR", t } = {}) {
  const defaultWeekdays = locale && locale.startsWith("en")
    ? ["M", "T", "W", "T", "F", "S", "S"]
    : ["S", "T", "Q", "Q", "S", "S", "D"];
  const catalogWeekdays = t?.("weekdays.short");
  const weekdayLabels = Array.isArray(catalogWeekdays) ? catalogWeekdays : defaultWeekdays;

  // Find max download for opacity scaling
  let maxDownload = 0;
  const cellMap = new Map();
  for (const c of cells || []) {
    if (c && typeof c.weekday === "number" && typeof c.hour === "number") {
      cellMap.set(`${c.weekday}_${c.hour}`, c);
      if (Number.isFinite(c.download) && c.download > maxDownload) {
        maxDownload = c.download;
      }
    }
  }

  const rows = [];
  for (let w = 0; w < 7; w++) {
    const dayLabel = weekdayLabels[w] || String(w);
    const dayCells = [];

    for (let h = 0; h < 24; h++) {
      const cell = cellMap.get(`${w}_${h}`);
      let bg = "#22264A";
      let title = `${dayLabel} ${String(h).padStart(2, "0")}h —`;
      let dlAttr = "";

      if (cell && Number.isFinite(cell.download) && cell.download > 0) {
        const ratio = maxDownload > 0 ? Math.min(1, Math.max(0, cell.download / maxDownload)) : 0;
        const opacity = 0.18 + ratio * 0.82;
        bg = `rgba(139, 124, 255, ${opacity.toFixed(2)})`;
        const dlStr = formatMetric(cell.download, locale, "download");
        const countStr = cell.count != null ? ` (${cell.count})` : "";
        title = `${dayLabel} ${String(h).padStart(2, "0")}:00 · ${dlStr} Mbps${countStr}`;
        dlAttr = ` data-download="${cell.download}"`;
      }

      dayCells.push(
        `<div class="heatmap-cell" style="background: ${bg};" data-weekday="${w}" data-hour="${h}"${dlAttr} title="${escapeHtml(title)}"></div>`
      );
    }

    rows.push(`<div class="heatmap-row">
      <span class="heatmap-weekday">${escapeHtml(dayLabel)}</span>
      <div class="heatmap-row-cells">
        ${dayCells.join("")}
      </div>
    </div>`);
  }

  return `<div class="heatmap">
  <div class="heatmap-grid">
    ${rows.join("\n")}
  </div>
  <div class="heatmap-hours">
    <span class="heatmap-hour">00h</span>
    <span class="heatmap-hour">06h</span>
    <span class="heatmap-hour">12h</span>
    <span class="heatmap-hour">18h</span>
  </div>
</div>`;
}

/**
 * 24-group SVG bar chart comparing average download and upload per hour of day.
 *
 * @param {Array<{ hour: number, download: number|null, upload: number|null, ping?: number|null, jitter?: number|null, count?: number }>} hourly
 * @param {object} opts
 * @param {string} opts.locale
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t]
 * @returns {string} SVG string
 */
export function renderHourlyChart(hourly = [], { locale = "pt-BR", t } = {}) {
  const width = 612;
  const height = 200;
  const plotTop = 30;
  const plotBottom = 160;
  const plotHeight = plotBottom - plotTop;

  // Build map of hour -> bucket
  const hourMap = new Map();
  let maxSpeed = 1;
  for (const b of hourly || []) {
    if (b && typeof b.hour === "number") {
      hourMap.set(b.hour, b);
      if (Number.isFinite(b.download) && b.download > maxSpeed) {
        maxSpeed = b.download;
      }
      if (Number.isFinite(b.upload) && b.upload > maxSpeed) {
        maxSpeed = b.upload;
      }
    }
  }
  const maxY = maxSpeed * 1.15;

  // 4 horizontal grid lines
  const gridLines = [
    plotTop,
    plotTop + plotHeight * (1 / 3),
    plotTop + plotHeight * (2 / 3),
    plotBottom,
  ].map((y) => `<line x1="20" y1="${y.toFixed(1)}" x2="${width - 20}" y2="${y.toFixed(1)}" stroke="#262A47" stroke-width="1" />`).join("\n    ");

  const padLeft = 20;
  const padRight = 20;
  const plotWidth = width - padLeft - padRight;
  const slotWidth = plotWidth / 24;
  const barWidth = Math.max(3, Math.min(8, (slotWidth - 6) / 2));

  const groups = [];
  const axisLabels = [];

  for (let h = 0; h < 24; h++) {
    const bucket = hourMap.get(h) || { hour: h, download: 0, upload: 0 };
    const slotX = padLeft + h * slotWidth;

    const dlVal = Math.max(0, Number(bucket.download) || 0);
    const dlHeight = maxY > 0 ? (dlVal / maxY) * plotHeight : 0;
    const dlY = plotBottom - dlHeight;
    const dlX = slotX + (slotWidth / 2) - barWidth - 1;

    const ulVal = Math.max(0, Number(bucket.upload) || 0);
    const ulHeight = maxY > 0 ? (ulVal / maxY) * plotHeight : 0;
    const ulY = plotBottom - ulHeight;
    const ulX = slotX + (slotWidth / 2) + 1;

    const dlTooltip = formatMetric(bucket.download, locale, "download");
    const ulTooltip = formatMetric(bucket.upload, locale, "upload");
    const groupTitle = `${String(h).padStart(2, "0")}h · DL: ${dlTooltip} Mbps, UL: ${ulTooltip} Mbps`;

    groups.push(`<g class="hour-group" data-hour="${h}">
      <title>${escapeHtml(groupTitle)}</title>
      <rect class="bar-download" x="${dlX.toFixed(1)}" y="${dlY.toFixed(1)}" width="${barWidth.toFixed(1)}" height="${dlHeight.toFixed(1)}" fill="#8B7CFF" rx="2" />
      <rect class="bar-upload" x="${ulX.toFixed(1)}" y="${ulY.toFixed(1)}" width="${barWidth.toFixed(1)}" height="${ulHeight.toFixed(1)}" fill="#3FD6C2" rx="2" />
    </g>`);

    if (h % 3 === 0) {
      const labelX = slotX + slotWidth / 2;
      axisLabels.push(
        `<text x="${labelX.toFixed(1)}" y="180" text-anchor="middle" font-family="'JetBrains Mono', monospace" font-size="10" fill="#8A8DB0">${String(h).padStart(2, "0")}h</text>`
      );
    }
  }

  return `<svg viewBox="0 0 ${width} ${height}" class="hourly-chart" role="img" aria-label="Hourly chart">
    ${gridLines}
    ${groups.join("\n    ")}
    ${axisLabels.join("\n    ")}
  </svg>`;
}

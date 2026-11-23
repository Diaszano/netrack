import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";

import { createTranslator } from "../i18n.js";
import { renderOverview } from "../views/overview.js";
import { renderStatistics } from "../views/statistics.js";
import { renderHistory } from "../views/history.js";

const ptBR = JSON.parse(
  fs.readFileSync(new URL("../locales/pt-BR.json", import.meta.url), "utf8")
);
const en = JSON.parse(
  fs.readFileSync(new URL("../locales/en.json", import.meta.url), "utf8")
);
const tPt = createTranslator(ptBR, en);

function createMockState(overrides = {}) {
  return {
    route: "overview",
    locale: "pt-BR",
    status: {
      running: false,
      paused: false,
      nextRun: null,
    },
    system: {
      version: "1.0.0",
      uptimeSecs: 3600,
      cpuPercent: 12,
      rssBytes: 35000000,
      tz: "America/Sao_Paulo",
    },
    config: {
      expectedDownload: 600,
      expectedUpload: 300,
      expectedPing: 20,
    },
    recentTests: [],
    overviewStats: {
      total: 10,
      failed: 1,
      availability: 90.0,
      download: { avg: 550, min: 350, max: 590, stdDev: 40, consistency: 92.7 },
      upload: { avg: 250, min: 180, max: 280, stdDev: 20, consistency: 92.0 },
      ping: { avg: 14, min: 10, max: 25, stdDev: 3, consistency: 78.6 },
      jitter: { avg: 2.1, min: 1.0, max: 5.0, stdDev: 0.8, consistency: 61.9 },
      hourly: [],
      heatmap: [],
      series: [
        { t: 1762516800000, download: 587, upload: 274, ping: 11 },
      ],
    },
    statsRange: {
      preset: "7d",
      from: "2026-11-01",
      to: "2026-11-07",
    },
    statsData: null,
    history: {
      rows: [],
      hasMore: false,
      loading: false,
    },
    servers: {
      interfaces: [],
      libreServers: [],
      libreError: null,
    },
    banner: null,
    ...overrides,
  };
}

test("overview_renders_gauges_chart_heatmap_and_recent_rows", () => {
  const state = createMockState({
    recentTests: [
      {
        id: 101,
        createdAt: 1762516800000,
        kind: "auto",
        provider: "cloudflare",
        serverName: "São Paulo",
        serverHost: "gru.cloudflare.com",
        download: 587,
        upload: 274,
        ping: 11,
        jitter: 1.8,
        duration: 12.3,
        error: null,
      },
      {
        id: 100,
        createdAt: 1762513200000,
        kind: "auto",
        provider: "cloudflare",
        serverName: "São Paulo",
        serverHost: "gru.cloudflare.com",
        download: null,
        upload: null,
        ping: null,
        jitter: null,
        duration: 2.1,
        error: "timeout",
      },
    ],
  });

  const html = renderOverview(state, { t: tPt });

  // 1. Gauges contain values from newest non-failed row
  assert.ok(html.includes("587"), "Must render download value 587");
  assert.ok(html.includes("274"), "Must render upload value 274");
  assert.ok(html.includes("11"), "Must render ping value 11");

  // 2. Recent tests table contains ok and falha badges
  assert.ok(html.includes("ok"), "Must render ok status badge");
  assert.ok(html.includes("falha"), "Must render falha status badge");

  // 3. Failed row displays "—" for metrics
  assert.ok(html.includes("—"), "Must render — placeholder for failed row metrics");

  // 4. Action button data attribute
  assert.ok(html.includes('data-action="run-test"'), "Must include run-test button");
  assert.ok(!html.includes("disabled"), "Run-test button should not be disabled when not running");

  // 5. Speed chart and heatmap present
  assert.ok(html.includes("speed-chart") || html.includes("speed-chart-container"), "Must render speed chart container");
  assert.ok(html.includes("heatmap") || html.includes("heatmap-container"), "Must render heatmap container");
});

test("overview_and_stats_render_when_history_is_empty", () => {
  const state = createMockState({
    recentTests: [],
    overviewStats: {
      total: 0,
      failed: 0,
      availability: null,
      download: { avg: null, min: null, max: null, stdDev: null, consistency: null },
      upload: { avg: null, min: null, max: null, stdDev: null, consistency: null },
      ping: { avg: null, min: null, max: null, stdDev: null, consistency: null },
      jitter: { avg: null, min: null, max: null, stdDev: null, consistency: null },
      hourly: [],
      heatmap: [],
      series: [],
    },
    statsData: null,
  });

  const overviewHtml = renderOverview(state, { t: tPt });
  assert.ok(!overviewHtml.includes("NaN"), "renderOverview must not contain NaN");
  assert.ok(!overviewHtml.includes("undefined"), "renderOverview must not contain undefined");
  assert.ok(overviewHtml.includes("—"), "renderOverview must render em-dash placeholders");

  const statsHtml = renderStatistics(state, { t: tPt });
  assert.ok(!statsHtml.includes("NaN"), "renderStatistics must not contain NaN");
  assert.ok(!statsHtml.includes("undefined"), "renderStatistics must not contain undefined");
  assert.ok(statsHtml.includes("—"), "renderStatistics must render em-dash placeholders");
});

test("history_and_recent_escape_html_in_server_and_error", () => {
  const maliciousRow = {
    id: 999,
    createdAt: 1762516800000,
    kind: "auto",
    provider: "librespeed",
    serverName: "<img src=x onerror=alert(1)>",
    serverHost: "evil.example.com",
    download: null,
    upload: null,
    ping: null,
    jitter: null,
    duration: 1.0,
    error: "<script>evil()</script>",
  };

  const state = createMockState({
    recentTests: [maliciousRow],
    history: {
      rows: [maliciousRow],
      hasMore: false,
      loading: false,
    },
  });

  const overviewHtml = renderOverview(state, { t: tPt });
  assert.ok(!overviewHtml.includes("<script>evil()</script>"), "overview must never contain raw <script>");
  assert.ok(!overviewHtml.includes("<img src=x"), "overview must never contain raw <img");
  assert.ok(overviewHtml.includes("&lt;img"), "overview must contain escaped &lt;img");
  assert.ok(overviewHtml.includes("&lt;script&gt;"), "overview must contain escaped &lt;script&gt;");

  const historyHtml = renderHistory(state, { t: tPt });
  assert.ok(!historyHtml.includes("<script>evil()</script>"), "history must never contain raw <script>");
  assert.ok(!historyHtml.includes("<img src=x"), "history must never contain raw <img");
  assert.ok(historyHtml.includes("&lt;img"), "history must contain escaped &lt;img");
  assert.ok(historyHtml.includes("&lt;script&gt;"), "history must contain escaped &lt;script&gt;");
});

test("history_shows_load_more_only_when_has_more", () => {
  const stateWithMore = createMockState({
    history: {
      rows: [
        {
          id: 1,
          createdAt: 1762516800000,
          kind: "auto",
          provider: "cloudflare",
          download: 500,
          upload: 250,
          ping: 15,
          jitter: 2.0,
          duration: 10.0,
          error: null,
        },
      ],
      hasMore: true,
      loading: false,
    },
  });
  const htmlWithMore = renderHistory(stateWithMore, { t: tPt });
  assert.ok(
    htmlWithMore.includes('data-action="load-more-history"'),
    "Must render load-more button when hasMore is true"
  );

  const stateWithoutMore = createMockState({
    history: {
      rows: [
        {
          id: 1,
          createdAt: 1762516800000,
          kind: "auto",
          provider: "cloudflare",
          download: 500,
          upload: 250,
          ping: 15,
          jitter: 2.0,
          duration: 10.0,
          error: null,
        },
      ],
      hasMore: false,
      loading: false,
    },
  });
  const htmlWithoutMore = renderHistory(stateWithoutMore, { t: tPt });
  assert.ok(
    !htmlWithoutMore.includes('data-action="load-more-history"'),
    "Must NOT render load-more button when hasMore is false"
  );
});

test("overview_status_pill_and_running_button_state", () => {
  // 1. When running: button is disabled
  const runningState = createMockState({
    status: { running: true, paused: false, nextRun: null },
  });
  const runningHtml = renderOverview(runningState, { t: tPt });
  assert.ok(
    runningHtml.includes('data-action="run-test"') && runningHtml.includes("disabled"),
    "run-test button must be disabled when status.running is true"
  );

  // 2. When paused: shows paused pill
  const pausedState = createMockState({
    status: { running: false, paused: true, nextRun: null },
  });
  const pausedHtml = renderOverview(pausedState, { t: tPt });
  assert.ok(
    pausedHtml.includes("Agendamento pausado"),
    "overview must display paused pill when paused"
  );

  // 3. When nextRun is set: shows formatted time
  const nextRunState = createMockState({
    status: { running: false, paused: false, nextRun: 1762520400000 },
  });
  const nextRunHtml = renderOverview(nextRunState, { t: tPt });
  assert.ok(
    nextRunHtml.includes("Próximo teste"),
    "overview must display next run notice when nextRun is set"
  );

  // 4. When status.paused is nullish, falls back to config.paused
  const fallbackState = createMockState({
    status: { running: false, nextRun: null },
    config: { paused: true },
  });
  const fallbackHtml = renderOverview(fallbackState, { t: tPt });
  assert.ok(
    fallbackHtml.includes("Agendamento pausado"),
    "overview must display paused pill when status.paused is nullish and config.paused is true"
  );
});

test("statistics_preset_bar_and_kpis_and_metric_cards", () => {
  const state = createMockState({
    statsRange: {
      preset: "30d",
      from: "2026-10-08",
      to: "2026-11-07",
    },
    statsData: {
      total: 50,
      failed: 2,
      availability: 96.0,
      download: { avg: 520, min: 310, max: 605, stdDev: 45, consistency: 91.3 },
      upload: { avg: 260, min: 150, max: 310, stdDev: 25, consistency: 90.4 },
      ping: { avg: 12, min: 8, max: 28, stdDev: 3.5, consistency: 70.8 },
      jitter: { avg: 1.9, min: 0.8, max: 4.2, stdDev: 0.7, consistency: 63.2 },
      hourly: [],
      heatmap: [],
      series: [],
    },
  });

  const html = renderStatistics(state, { t: tPt });

  // Preset buttons
  assert.ok(html.includes('class="btn btn-preset active" data-action="stats-preset" data-preset="30d"'));
  assert.ok(html.includes('data-action="stats-preset" data-preset="24h"'));
  assert.ok(html.includes('data-action="stats-preset" data-preset="7d"'));
  assert.ok(html.includes('data-action="stats-preset" data-preset="365d"'));

  // Date inputs
  assert.ok(html.includes('name="from"'));
  assert.ok(html.includes('value="2026-10-08"'));
  assert.ok(html.includes('name="to"'));
  assert.ok(html.includes('value="2026-11-07"'));

  // KPIs
  assert.ok(html.includes("50"), "Total tests 50");
  assert.ok(html.includes("2"), "Failures 2");
  assert.ok(html.includes("96") && html.includes("%"), "Availability 96%");

  // 4 metric summary cards metrics
  assert.ok(html.includes("520"), "Download avg 520");
  assert.ok(html.includes("260"), "Upload avg 260");
  assert.ok(html.includes("12"), "Ping avg 12");
  assert.ok(html.includes("1,9") || html.includes("1.9"), "Jitter avg 1.9");
  assert.ok(html.includes("91,3%") || html.includes("91.3%"), "Consistency 91.3%");
});

test("history_renders_delete_button_and_kind_badges", () => {
  const state = createMockState({
    history: {
      rows: [
        {
          id: 42,
          createdAt: 1762516800000,
          kind: "custom",
          provider: "librespeed",
          serverName: "Local Node",
          serverHost: "192.168.1.10",
          download: 940,
          upload: 920,
          ping: 2,
          jitter: 0.3,
          duration: 15.5,
          error: null,
        },
      ],
      hasMore: false,
      loading: false,
    },
  });

  const html = renderHistory(state, { t: tPt });

  assert.ok(html.includes('data-action="delete-test" data-id="42"'), "Delete button with data-id 42");
  assert.ok(html.includes("badge-manual") || html.includes("badge-custom"), "Must render manual badge for custom kind");
  assert.ok(html.includes("940"), "Download metric 940");
  assert.ok(html.includes("15,5") || html.includes("15.5"), "Duration 15.5");
});

import { renderServerCell, renderStatusBadge } from "../views/test-row.js";

test("renderServerCell escapes and joins name and host", () => {
  const html = renderServerCell({ provider: "cloudflare", serverName: "<b>SP</b>", serverHost: "h" });
  assert.ok(html.includes("&lt;b&gt;SP&lt;/b&gt; (h)"));
});

test("renderStatusBadge has no inline color", () => {
  assert.ok(!renderStatusBadge({}, { t: tPt }).includes("style="));
  assert.ok(!renderStatusBadge({ error: "x" }, { t: tPt }).includes("style="));
});

test("renderStatistics lists each provider/server and escapes names", () => {
  const state = createMockState({
    route: "statistics",
    statsData: {
      total: 3,
      failed: 1,
      byServer: [
        { provider: "ookla", serverName: "<b>SP</b>", total: 2, failed: 0, download: 150, upload: 40, ping: 9 },
        { provider: "ookla", serverName: null, total: 1, failed: 1, download: null, upload: null, ping: null },
      ],
    },
  });
  const html = renderStatistics(state, { t: tPt });
  assert.ok(html.includes("Por provedor e servidor"));
  assert.ok(html.includes("&lt;b&gt;SP&lt;/b&gt;"));
  assert.ok(html.includes("sem servidor"));
  assert.ok(!html.includes("NaN"));
});

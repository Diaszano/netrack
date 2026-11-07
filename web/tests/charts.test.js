import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";

import { createTranslator } from "../i18n.js";
import {
  renderGaugeCard,
  renderSpeedChart,
  renderHeatmap,
  renderHourlyChart,
} from "../charts.js";

const ptBR = JSON.parse(
  fs.readFileSync(new URL("../locales/pt-BR.json", import.meta.url), "utf8")
);
const en = JSON.parse(
  fs.readFileSync(new URL("../locales/en.json", import.meta.url), "utf8")
);
const tPt = createTranslator(ptBR, en);
const tEn = createTranslator(en, ptBR);

test("gauge_card_renders_penpot_metrics_and_plan_pct", () => {
  const html = renderGaugeCard({
    metric: "download",
    value: 587,
    unit: "Mbps",
    color: "#8B7CFF",
    expected: 600,
    avg: 552,
    min: 371,
    locale: "pt-BR",
    t: tPt,
  });

  assert.ok(html.includes("gauge-card"), "Must contain gauge-card class");
  assert.ok(html.includes("587"), "Must render centered value 587");
  assert.ok(html.includes("Mbps"), "Must render unit Mbps");
  assert.ok(html.includes("98% do plano"), "Must render 98% do plano subtitle");
  assert.ok(html.includes("méd 552"), "Must render méd 552");
  assert.ok(html.includes("mín 371"), "Must render mín 371");
  assert.ok(html.includes("#8B7CFF"), "Must render value stroke color #8B7CFF");
  assert.ok(html.includes("#262A47"), "Must render track stroke color #262A47");
});

test("ping_gauge_renders_jitter_and_max", () => {
  const html = renderGaugeCard({
    metric: "ping",
    value: 11,
    unit: "ms",
    color: "#FF8FB1",
    expected: 25,
    avg: 14,
    max: 31,
    jitter: 1.8,
    locale: "pt-BR",
    t: tPt,
  });

  assert.ok(html.includes("11"), "Must render centered value 11");
  assert.ok(html.includes("ms"), "Must render unit ms");
  assert.ok(html.includes("jitter 1,8"), "Must render jitter 1,8");
  assert.ok(html.includes("méd 14"), "Must render méd 14");
  assert.ok(html.includes("máx 31"), "Must render máx 31");
  assert.ok(html.includes("#FF8FB1"), "Must render ping color #FF8FB1");
});

test("gauge_and_charts_handle_empty_and_zero_expected", () => {
  const emptyGauge = renderGaugeCard({
    metric: "download",
    value: null,
    unit: "Mbps",
    color: "#8B7CFF",
    expected: 0,
    avg: null,
    min: null,
    locale: "pt-BR",
    t: tPt,
  });

  assert.ok(!emptyGauge.includes("NaN"), "Empty gauge must not contain NaN");
  assert.ok(!emptyGauge.includes("Infinity"), "Empty gauge must not contain Infinity");
  assert.ok(emptyGauge.includes("—"), "Empty gauge must contain placeholder em-dash");

  const emptySpeed = renderSpeedChart([], {
    locale: "pt-BR",
    tz: "America/Sao_Paulo",
    t: tPt,
  });
  assert.ok(!emptySpeed.includes("NaN"), "Empty speed chart must not contain NaN");
  assert.ok(!emptySpeed.includes("Infinity"), "Empty speed chart must not contain Infinity");
  assert.ok(emptySpeed.includes("chart-empty"), "Empty speed chart must render .chart-empty message");

  const singlePoint = {
    t: 1773106200000,
    download: 450.5,
    upload: 120.2,
    ping: 15.3,
  };
  const singleSpeed = renderSpeedChart([singlePoint], {
    locale: "pt-BR",
    tz: "America/Sao_Paulo",
    t: tPt,
  });
  assert.ok(!singleSpeed.includes("NaN"), "Single-point speed chart must not contain NaN");
  assert.ok(!singleSpeed.includes("Infinity"), "Single-point speed chart must not contain Infinity");
  assert.ok(singleSpeed.includes("chart-point"), "Single-point speed chart must render visible dots");
  assert.ok(singleSpeed.includes("data-dl=\"450.5\""), "Must contain data-dl attribute");

  const emptyHeatmap = renderHeatmap([], {
    locale: "pt-BR",
    t: tPt,
  });
  assert.ok(!emptyHeatmap.includes("NaN"), "Empty heatmap must not contain NaN");
  assert.ok(!emptyHeatmap.includes("Infinity"), "Empty heatmap must not contain Infinity");

  const empty24 = Array.from({ length: 24 }, (_, hour) => ({
    hour,
    download: 0,
    upload: 0,
    ping: 0,
    jitter: 0,
    count: 0,
  }));
  const emptyHourly = renderHourlyChart(empty24, {
    locale: "pt-BR",
    t: tPt,
  });
  assert.ok(!emptyHourly.includes("NaN"), "Empty hourly chart must not contain NaN");
  assert.ok(!emptyHourly.includes("Infinity"), "Empty hourly chart must not contain Infinity");
});

test("heatmap_maps_weekday_hour_and_scales_opacity", () => {
  const htmlPt = renderHeatmap(
    [{ weekday: 0, hour: 14, download: 500, count: 3 }],
    { locale: "pt-BR", t: tPt }
  );

  // Must render 168 cells
  const cellMatches = htmlPt.match(/class="[^"]*heat(?:map)?-cell[^"]*"/g) || [];
  assert.strictEqual(cellMatches.length, 168, "Must render exactly 168 (7x24) heatmap cells");

  // Weekday 0, hour 14 tooltip contains 500 and Mbps
  assert.ok(htmlPt.includes("500"), "Must mark (0, 14) with 500 in its tooltip/attributes");
  assert.ok(htmlPt.includes("Mbps"), "Must contain unit Mbps in tooltip");

  // Empty cells get #22264A
  assert.ok(htmlPt.includes("#22264A"), "Empty cells must use #22264A");

  // Check English weekdays
  const htmlEn = renderHeatmap([], { locale: "en", t: tEn });
  assert.ok(htmlEn.includes("M"), "English heatmap must contain Monday abbreviation M");
  assert.ok(htmlEn.includes("00h") || htmlEn.includes("00:00") || htmlEn.includes("00"), "Must contain 00 hour label");
  assert.ok(htmlEn.includes("12h") || htmlEn.includes("12:00") || htmlEn.includes("12"), "Must contain 12 hour label");
});

test("speed_chart_renders_svg_areas_lines_and_tz_time_labels", () => {
  const series = [
    { t: 1773106200000, download: 500, upload: 100, ping: 12 }, // 2026-03-09 22:30 in America/Sao_Paulo
    { t: 1773109800000, download: 600, upload: 150, ping: 10 }, // 2026-03-09 23:30 in America/Sao_Paulo
    { t: 1773113400000, download: 400, upload: 80, ping: 14 },  // 2026-03-10 00:30 in America/Sao_Paulo
  ];

  const svg = renderSpeedChart(series, {
    locale: "pt-BR",
    tz: "America/Sao_Paulo",
    t: tPt,
  });

  assert.ok(svg.includes("<svg"), "Must render SVG root element");
  assert.ok(svg.includes('viewBox="0 0 612 210"'), "Must have viewBox 0 0 612 210");
  assert.ok(svg.includes('class="speed-chart"'), "Must have class speed-chart");
  assert.ok(svg.includes("#262A47"), "Must render grid lines with #262A47");
  assert.ok(svg.includes("#8B7CFF"), "Must render download color #8B7CFF");
  assert.ok(svg.includes("#3FD6C2"), "Must render upload color #3FD6C2");
  assert.ok(svg.includes("fill-opacity=\"0.15\""), "Download area must have fill-opacity 0.15");
  assert.ok(svg.includes("fill-opacity=\"0.12\""), "Upload area must have fill-opacity 0.12");

  // Check server timezone time labels
  assert.ok(svg.includes("22:30"), "Must render time formatted in America/Sao_Paulo (22:30)");
  assert.ok(svg.includes("chart-point"), "Must render .chart-point circles");
  assert.ok(svg.includes('data-t="1773106200000"'), "Must render data-t attribute");
});

test("hourly_chart_renders_24_hour_groups_download_and_upload_bars", () => {
  const hourly = Array.from({ length: 24 }, (_, hour) => ({
    hour,
    download: hour * 20,
    upload: hour * 5,
    ping: 15,
    jitter: 1.2,
    count: 2,
  }));

  const svg = renderHourlyChart(hourly, {
    locale: "pt-BR",
    t: tPt,
  });

  assert.ok(svg.includes("<svg"), "Must render SVG element");
  assert.ok(svg.includes("#8B7CFF"), "Must render download bars #8B7CFF");
  assert.ok(svg.includes("#3FD6C2"), "Must render upload bars #3FD6C2");

  // Check that 24 hour groups are present
  for (let h = 0; h < 24; h++) {
    assert.ok(svg.includes(`data-hour="${h}"`), `Must include data-hour="${h}"`);
  }
});

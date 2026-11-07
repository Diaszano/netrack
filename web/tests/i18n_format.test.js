import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";

import {
  escapeHtml,
  resolveLocale,
  createTranslator,
} from "../i18n.js";

import {
  formatNumber,
  formatMetric,
  formatTime,
  formatDateTime,
  formatDateInput,
  dateRangeForPreset,
  formatBytes,
  computePlanProgress,
} from "../format.js";

test("locale_catalogs_have_identical_keys", () => {
  const ptBR = JSON.parse(
    fs.readFileSync(new URL("../locales/pt-BR.json", import.meta.url), "utf8")
  );
  const en = JSON.parse(
    fs.readFileSync(new URL("../locales/en.json", import.meta.url), "utf8")
  );

  const ptKeys = Object.keys(ptBR).sort();
  const enKeys = Object.keys(en).sort();

  assert.deepStrictEqual(ptKeys, enKeys, "pt-BR and en catalogs must have identical sorted keys");
  assert.ok(ptKeys.length > 30, `Expected at least 30 translation keys, got ${ptKeys.length}`);

  // Check _meta
  assert.strictEqual(typeof ptBR._meta?.name, "string");
  assert.ok(ptBR._meta.name.trim().length > 0);
  assert.strictEqual(typeof en._meta?.name, "string");
  assert.ok(en._meta.name.trim().length > 0);

  // Check all non-_meta keys have valid non-empty values
  for (const key of ptKeys) {
    if (key === "_meta") continue;
    const ptVal = ptBR[key];
    const enVal = en[key];

    if (Array.isArray(ptVal)) {
      assert.ok(Array.isArray(enVal), `Key "${key}" should be an array in en catalog too`);
      assert.strictEqual(ptVal.length, enVal.length, `Array length mismatch for key "${key}"`);
      assert.ok(ptVal.length > 0, `Array "${key}" in pt-BR cannot be empty`);
      for (const item of ptVal) {
        assert.strictEqual(typeof item, "string", `Item in "${key}" must be string`);
        assert.ok(item.length > 0, `Item in "${key}" cannot be empty`);
      }
      for (const item of enVal) {
        assert.strictEqual(typeof item, "string", `Item in "${key}" must be string`);
        assert.ok(item.length > 0, `Item in "${key}" cannot be empty`);
      }
    } else {
      assert.strictEqual(
        typeof ptVal,
        "string",
        `Key "${key}" in pt-BR must be a string`
      );
      assert.ok(
        ptVal.trim().length > 0,
        `Key "${key}" in pt-BR cannot be empty`
      );
      assert.strictEqual(
        typeof enVal,
        "string",
        `Key "${key}" in en must be a string`
      );
      assert.ok(
        enVal.trim().length > 0,
        `Key "${key}" in en cannot be empty`
      );
    }
  }
});

test("resolve_locale_priority_and_subtag_fallback", () => {
  // Saved preference takes precedence if valid and not "auto"
  assert.strictEqual(resolveLocale({ saved: "en" }), "en");
  assert.strictEqual(resolveLocale({ saved: "pt-BR" }), "pt-BR");

  // When saved is "auto", fall back to navigatorLanguages
  assert.strictEqual(
    resolveLocale({ saved: "auto", navigatorLanguages: ["pt-PT", "en-US"] }),
    "pt-BR"
  );

  // Subtag fallback for English dialects
  assert.strictEqual(
    resolveLocale({ saved: "auto", navigatorLanguages: ["en-US", "pt-BR"] }),
    "en"
  );
  assert.strictEqual(
    resolveLocale({ saved: "auto", navigatorLanguages: ["en-GB"] }),
    "en"
  );

  // Unsupported language falls back to defaultLocale ("pt-BR")
  assert.strictEqual(
    resolveLocale({ navigatorLanguages: ["fr-FR"] }),
    "pt-BR"
  );

  // Invalid saved falls back to navigatorLanguages
  assert.strictEqual(
    resolveLocale({ saved: "xyz", navigatorLanguages: ["en"] }),
    "en"
  );

  // Empty parameters fallback to default
  assert.strictEqual(resolveLocale(), "pt-BR");
  assert.strictEqual(resolveLocale({}), "pt-BR");
});

test("translator_interpolates_and_falls_back", () => {
  const ptBR = JSON.parse(
    fs.readFileSync(new URL("../locales/pt-BR.json", import.meta.url), "utf8")
  );
  const en = JSON.parse(
    fs.readFileSync(new URL("../locales/en.json", import.meta.url), "utf8")
  );

  const tPt = createTranslator(ptBR, en);
  const tEn = createTranslator(en, ptBR);

  assert.strictEqual(tPt("overview.planPercent", { pct: 98 }), "98% do plano");
  assert.strictEqual(tEn("overview.planPercent", { pct: 98 }), "98% of plan");

  // Fallback dictionary
  const tFallback = createTranslator({ "existing": "Original {val}" }, { "onlyFallback": "Fallback {val}" });
  assert.strictEqual(tFallback("existing", { val: "1" }), "Original 1");
  assert.strictEqual(tFallback("onlyFallback", { val: "2" }), "Fallback 2");
  assert.strictEqual(tFallback("missing.key"), "missing.key");

  // Array values return as-is
  assert.deepStrictEqual(tPt("weekdays.short"), ["S", "T", "Q", "Q", "S", "S", "D"]);
  assert.deepStrictEqual(tEn("weekdays.short"), ["M", "T", "W", "T", "F", "S", "S"]);

  // escapeHtml helper
  assert.strictEqual(
    escapeHtml("<script>alert('xss & \"foo\"')</script>"),
    "&lt;script&gt;alert(&#39;xss &amp; &quot;foo&quot;&#39;)&lt;/script&gt;"
  );
  assert.strictEqual(escapeHtml(null), "");
  assert.strictEqual(escapeHtml(undefined), "");
  assert.strictEqual(escapeHtml(123), "123");
  assert.strictEqual(escapeHtml("hello"), "hello");
});

test("formatters_and_presets_use_server_tz", () => {
  const ms = 1773106200000; // 2026-03-10T01:30:00Z -> 2026-03-09 22:30 in America/Sao_Paulo (UTC-3)
  const tz = "America/Sao_Paulo";

  assert.strictEqual(formatTime(ms, "pt-BR", tz), "22:30");
  assert.strictEqual(formatDateInput(ms, tz), "2026-03-09");
  assert.strictEqual(formatDateTime(ms, "pt-BR", tz), "09/03/2026, 22:30");

  assert.strictEqual(formatNumber(1.8, "pt-BR", 1), "1,8");
  assert.strictEqual(formatNumber(1.8, "en", 1), "1.8");
  assert.strictEqual(formatNumber(null, "pt-BR", 1), "—");
  assert.strictEqual(formatNumber(undefined, "pt-BR", 1), "—");
  assert.strictEqual(formatNumber(NaN, "pt-BR", 1), "—");
  assert.strictEqual(formatNumber(Infinity, "pt-BR", 1), "—");

  // dateRangeForPreset
  const r24 = dateRangeForPreset("24h", ms, tz);
  assert.strictEqual(r24.to, "2026-03-09");
  assert.strictEqual(r24.from, "2026-03-08");

  const r7 = dateRangeForPreset("7d", ms, tz);
  assert.strictEqual(r7.to, "2026-03-09");
  assert.strictEqual(r7.from, "2026-03-03");

  const r30 = dateRangeForPreset("30d", ms, tz);
  assert.strictEqual(r30.to, "2026-03-09");
  assert.strictEqual(r30.from, "2026-02-08");

  const r365 = dateRangeForPreset("365d", ms, tz);
  assert.strictEqual(r365.to, "2026-03-09");
  assert.strictEqual(r365.from, "2025-03-10");

  // formatBytes
  assert.strictEqual(formatBytes(null, "pt-BR"), "—");
  assert.strictEqual(formatBytes(undefined, "pt-BR"), "—");
  assert.strictEqual(formatBytes(NaN, "pt-BR"), "—");
  assert.strictEqual(formatBytes(0, "pt-BR"), "0 MB");
  assert.strictEqual(formatBytes(25 * 1024 * 1024, "pt-BR"), "25 MB");
  assert.strictEqual(formatBytes(1.5 * 1024 * 1024 * 1024, "pt-BR"), "1,5 GB");
  assert.strictEqual(formatBytes(1.5 * 1024 * 1024 * 1024, "en"), "1.5 GB");

  // formatMetric
  assert.strictEqual(formatMetric(587, "pt-BR", "download"), "587");
  assert.strictEqual(formatMetric(1.8, "pt-BR", "jitter"), "1,8");
  assert.strictEqual(formatMetric(11, "pt-BR", "ping"), "11");
  assert.strictEqual(formatMetric(11.45, "pt-BR", "ping", 2), "11,45");
  assert.strictEqual(formatMetric(null, "pt-BR", "download"), "—");
});

test("plan_progress_handles_zero_and_null_expected", () => {
  const dl = computePlanProgress(587, 600, "download");
  assert.strictEqual(dl.percent, 98);
  assert.ok(Math.abs(dl.ratio - (587 / 600)) < 1e-6);

  assert.deepStrictEqual(computePlanProgress(587, 0, "download"), { percent: null, ratio: 0 });
  assert.deepStrictEqual(computePlanProgress(587, null, "download"), { percent: null, ratio: 0 });
  assert.deepStrictEqual(computePlanProgress(null, 600, "download"), { percent: null, ratio: 0 });
  assert.deepStrictEqual(computePlanProgress(NaN, 600, "download"), { percent: null, ratio: 0 });
  assert.deepStrictEqual(computePlanProgress(587, -10, "download"), { percent: null, ratio: 0 });

  // Ping: lower is better; ratio = Math.min(1, Math.max(0, expected / actual))
  const ping = computePlanProgress(10, 20, "ping");
  assert.strictEqual(ping.percent, 200);
  assert.strictEqual(ping.ratio, 1);

  const pingWorse = computePlanProgress(40, 20, "ping");
  assert.strictEqual(pingWorse.percent, 50);
  assert.strictEqual(pingWorse.ratio, 0.5);

  assert.deepStrictEqual(computePlanProgress(0, 20, "ping"), { percent: null, ratio: 0 });
  assert.deepStrictEqual(computePlanProgress(10, 0, "ping"), { percent: null, ratio: 0 });
  assert.deepStrictEqual(computePlanProgress(null, 20, "ping"), { percent: null, ratio: 0 });
  assert.deepStrictEqual(computePlanProgress(10, null, "ping"), { percent: null, ratio: 0 });
});

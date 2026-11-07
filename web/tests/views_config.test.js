import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";

import { createTranslator } from "../i18n.js";
import { createApiClient } from "../api.js";
import { renderServers } from "../views/servers.js";
import { renderSettings, saveConfigDiff } from "../views/settings.js";
import { renderNodes, renderIntegrations } from "../views/seams.js";

const ptBR = JSON.parse(
  fs.readFileSync(new URL("../locales/pt-BR.json", import.meta.url), "utf8")
);
const en = JSON.parse(
  fs.readFileSync(new URL("../locales/en.json", import.meta.url), "utf8")
);
const tPt = createTranslator(ptBR, en);
const locales = [
  { code: "en", name: "English" },
  { code: "pt-BR", name: "Português (Brasil)" },
];

function createMockState(overrides = {}) {
  return {
    route: "servers",
    locale: "pt-BR",
    status: {
      running: false,
      paused: false,
      nextRun: null,
    },
    system: {
      version: "1.0.0",
      uptimeSecs: 7325,
      cpuPercent: 14.5,
      rssBytes: 42000000,
      tz: "America/Sao_Paulo",
    },
    config: {
      provider: "librespeed",
      cron: "0 * * * *",
      scheduleOffset: true,
      retentionDays: 365,
      expectedMode: "manual",
      expectedDownload: 100,
      expectedUpload: 50,
      expectedPing: 25,
      libreServerId: "2",
      libreUrl: "https://librespeed.example.com",
      interface: "eth0",
      paused: false,
    },
    servers: {
      interfaces: [
        { name: "eth0", addrs: ["192.168.1.10"] },
        { name: "wlan0", addrs: ["192.168.1.50"] },
      ],
      libreServers: [
        { id: 1, name: "São Paulo, Brazil", sponsor: "Host A" },
        { id: 2, name: "Buenos Aires, Argentina", sponsor: "Host B" },
      ],
      libreError: null,
    },
    banner: null,
    ...overrides,
  };
}

test("servers_view_renders_providers_interfaces_and_libre_list", () => {
  const state = createMockState();
  const html = renderServers(state, { t: tPt });

  // 1. Providers: Cloudflare and LibreSpeed options
  assert.ok(html.includes("cloudflare"), "Must contain cloudflare provider option");
  assert.ok(html.includes("librespeed"), "Must contain librespeed provider option");
  assert.ok(html.includes("Cloudflare"), "Must render Cloudflare label");
  assert.ok(html.includes("LibreSpeed"), "Must render LibreSpeed label");

  // 2. Interfaces selector with eth0 (192.168.1.10)
  assert.ok(html.includes("eth0 (192.168.1.10)"), "Must render eth0 interface with IP address");
  assert.ok(html.includes("wlan0 (192.168.1.50)"), "Must render wlan0 interface with IP address");

  // 3. LibreSpeed server dropdown items
  assert.ok(html.includes("1 — São Paulo, Brazil"), "Must render LibreSpeed server id 1");
  assert.ok(html.includes("2 — Buenos Aires, Argentina"), "Must render LibreSpeed server id 2");

  // 4. Custom URL input with current value
  assert.ok(html.includes("https://librespeed.example.com"), "Must render current libreUrl");
});

test("servers_view_renders_warning_banner_when_libre_error_is_set", () => {
  const state = createMockState({
    servers: {
      interfaces: [{ name: "eth0", addrs: ["192.168.1.10"] }],
      libreServers: [],
      libreError: "Failed to fetch LibreSpeed server list (HTTP 502)",
    },
  });

  const html = renderServers(state, { t: tPt });
  assert.ok(html.includes("502") || html.includes("Failed to fetch"), "Must render error banner when libreError is set");
});

test("settings_view_renders_cron_retention_expected_and_locales", () => {
  const state = createMockState();
  const html = renderSettings(state, { t: tPt, locales });

  // 1. Schedule card values
  assert.ok(html.includes("0 * * * *"), "Must render cron value 0 * * * *");
  assert.ok(html.includes("365"), "Must render retentionDays value 365");

  // 2. Expected values
  assert.ok(html.includes("100"), "Must render expectedDownload value 100");
  assert.ok(html.includes("50"), "Must render expectedUpload value 50");
  assert.ok(html.includes("25"), "Must render expectedPing value 25");

  // 3. Language options
  assert.ok(html.includes("pt-BR"), "Must include pt-BR locale option");
  assert.ok(html.includes("en"), "Must include en locale option");
  assert.ok(html.includes("Português (Brasil)"), "Must render Português (Brasil) name");
  assert.ok(html.includes("English"), "Must render English name");
});

test("config_patch_maps_camel_to_snake_and_surfaces_400", async () => {
  const calls = [];
  const mockFetch = async (url, opts) => {
    calls.push({ url, method: opts.method, body: JSON.parse(opts.body) });
    if (url.includes("/cron")) {
      return {
        ok: false,
        status: 400,
        statusText: "Bad Request",
        json: async () => ({ error: "invalid value for cron: bad cron expression" }),
      };
    }
    return {
      ok: true,
      status: 200,
      json: async () => ({ ok: true }),
    };
  };

  const apiClient = createApiClient(mockFetch);
  const prevConfig = {
    scheduleOffset: true,
    cron: "0 * * * *",
    retentionDays: 365,
  };
  const formValues = {
    scheduleOffset: false,
    cron: "bad",
    retentionDays: 365, // unchanged
  };

  const result = await saveConfigDiff(prevConfig, formValues, apiClient);

  // 1. Should have called PATCH /api/config/schedule_offset with {"value":"false"}
  assert.equal(calls.length, 2);
  assert.equal(calls[0].url, "/api/config/schedule_offset");
  assert.deepEqual(calls[0].body, { value: "false" });

  // 2. Should have called PATCH /api/config/cron with {"value":"bad"}
  assert.equal(calls[1].url, "/api/config/cron");
  assert.deepEqual(calls[1].body, { value: "bad" });

  // 3. Should return { ok: false, error: ... }
  assert.equal(result.ok, false);
  assert.ok(
    result.error.includes("invalid value for cron"),
    `Expected error to contain 'invalid value for cron', got '${result.error}'`
  );
});

test("nodes_and_integrations_seams_render_system_metrics_and_placeholders", () => {
  const state = createMockState();
  const nodesHtml = renderNodes(state, { t: tPt });

  // 1. Local instance status card metrics
  assert.ok(nodesHtml.includes("1.0.0"), "Must render system version");
  assert.ok(nodesHtml.includes("America/Sao_Paulo"), "Must render system timezone");
  assert.ok(nodesHtml.includes("14,5") || nodesHtml.includes("14.5"), "Must render CPU percent");
  assert.ok(nodesHtml.includes("40 MB") || nodesHtml.includes("42 MB"), "Must render RAM in MB");

  // 2. Seam for remote nodes
  assert.ok(nodesHtml.includes("nodes.seamTitle") || nodesHtml.includes("Múltiplos nós"), "Must render multiple nodes seam title");

  // 3. Integrations empty state
  const intHtml = renderIntegrations(state, { t: tPt });
  assert.ok(intHtml.includes("integrations.emptyTitle") || intHtml.includes("Notificações e integrações"), "Must render integrations empty title");
  assert.ok(intHtml.includes("Discord") || intHtml.includes("Telegram"), "Must mention upcoming integrations");
});

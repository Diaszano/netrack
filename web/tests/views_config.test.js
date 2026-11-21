import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";

import { createTranslator } from "../i18n.js";
import { createApiClient } from "../api.js";
import { renderServers } from "../views/servers.js";
import { renderSettings, saveConfigDiff } from "../views/settings.js";
import { renderNodes } from "../views/nodes.js";
import { renderIntegrations } from "../views/integrations.js";
import {
  extractServersFormValues,
  extractSettingsFormValues,
  onServersSubmit,
  onSettingsSubmit,
  loadViewData,
} from "../app.js";

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

test("settings_view_leaves_backup_keep_empty_when_unknown", () => {
  const state = createMockState();
  delete state.config.backupKeep;
  const html = renderSettings(state, { t: tPt, locales });
  assert.ok(/name="backupKeep"[^>]*value=""/.test(html), "backupKeep must render empty when missing");
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

test("nodes_and_integrations_views_render_system_metrics_and_empty_states", () => {
  const state = createMockState();
  const nodesHtml = renderNodes(state, { t: tPt });

  // 1. Local instance status card metrics
  assert.ok(nodesHtml.includes("1.0.0"), "Must render system version");
  assert.ok(nodesHtml.includes("America/Sao_Paulo"), "Must render system timezone");
  assert.ok(nodesHtml.includes("14,5") || nodesHtml.includes("14.5"), "Must render CPU percent");
  assert.ok(nodesHtml.includes("40 MB") || nodesHtml.includes("42 MB"), "Must render RAM in MB");

  // 2. Remote nodes empty state
  assert.ok(nodesHtml.includes(tPt("nodes.seamTitle")), "Must render remote nodes empty state");

  // 3. Integrations empty state and channel selector
  const intHtml = renderIntegrations(state, { t: tPt });
  assert.ok(intHtml.includes(tPt("integrations.emptyTitle")), "Must render integrations empty title");
  assert.ok(intHtml.includes("Discord") && intHtml.includes("Telegram"), "Must include Discord and Telegram channels");
});

test("servers_form_preserves_librespeed_config_when_provider_is_cloudflare", async () => {
  const state = createMockState({
    config: {
      provider: "librespeed",
      libreServerId: "42",
      libreUrl: "https://my-libre.example.com",
      interface: "eth0",
    },
  });

  const fd = new FormData();
  fd.append("provider", "cloudflare");
  fd.append("interface", "eth0");
  // Note: libreServerId and libreUrl are omitted because they were disabled in the UI

  const extracted = extractServersFormValues(fd, state.config);
  assert.equal(extracted.provider, "cloudflare");
  assert.equal(extracted.libreServerId, "42", "Must preserve existing libreServerId");
  assert.equal(extracted.libreUrl, "https://my-libre.example.com", "Must preserve existing libreUrl");

  const patches = [];
  const mockApiClient = {
    async patchConfig(key, val) {
      patches.push({ key, val });
      return { ok: true };
    },
  };

  let rendered = false;
  let bannerUpdated = false;

  await onServersSubmit(fd, state, mockApiClient, {
    t: tPt,
    onSaveSuccess: () => { rendered = true; },
    onSaveError: () => { bannerUpdated = true; },
  });

  // Only provider should be patched; libre settings must not be overwritten or wiped
  assert.equal(patches.length, 1);
  assert.equal(patches[0].key, "provider");
  assert.equal(patches[0].val, "cloudflare");
  assert.equal(rendered, true);
  assert.equal(state.config.libreServerId, "42");
  assert.equal(state.config.libreUrl, "https://my-libre.example.com");
});

test("form_save_failure_does_not_call_render_preserving_form_state", async () => {
  const state = createMockState();
  const failingApiClient = {
    async patchConfig() {
      throw new Error("invalid value for cron");
    },
  };

  const fd = new FormData();
  fd.append("cron", "invalid");
  fd.append("retentionDays", "30");

  let rendered = false;
  let bannerUpdated = false;

  const result = await onSettingsSubmit(fd, state, failingApiClient, {
    t: tPt,
    onSaveSuccess: () => { rendered = true; },
    onSaveError: () => { bannerUpdated = true; },
  });

  // On save failure, render() must NOT be called so user's form inputs are preserved in DOM
  assert.equal(result.ok, false);
  assert.equal(rendered, false, "render() must not be called on save failure");
  assert.equal(bannerUpdated, true, "banner update must be called on save failure");
  assert.equal(state.banner?.kind, "error");
  assert.ok(state.banner?.text.includes("invalid value for cron"));

  // Also verify onServersSubmit on failure
  let serversRendered = false;
  let serversBannerUpdated = false;
  const sFd = new FormData();
  sFd.append("provider", "librespeed");

  const sResult = await onServersSubmit(sFd, state, failingApiClient, {
    t: tPt,
    onSaveSuccess: () => { serversRendered = true; },
    onSaveError: () => { serversBannerUpdated = true; },
  });

  assert.equal(sResult.ok, false);
  assert.equal(serversRendered, false, "render() must not be called on servers save failure");
  assert.equal(serversBannerUpdated, true, "banner update must be called on servers save failure");
  assert.equal(state.banner?.kind, "error");
});

test("settings_retention_days_does_not_silently_reset_empty_input_to_365", () => {
  const fd = new FormData();
  fd.append("cron", "0 * * * *");
  fd.append("retentionDays", "");

  const extracted = extractSettingsFormValues(fd);
  assert.equal(extracted.retentionDays, "", "Empty retentionDays must remain empty string and not be coerced to 365");
});

test("servers_view_renders_unavailable_option_for_unlisted_configured_interface", () => {
  // Case A: unlisted interface in pt-BR
  const stateUnlisted = createMockState({
    config: {
      provider: "cloudflare",
      interface: "eth3",
    },
    servers: {
      interfaces: [
        { name: "eth0", addrs: ["192.168.1.10"] },
        { name: "wlan0", addrs: ["192.168.1.50"] },
      ],
      libreServers: [],
      libreError: null,
    },
  });
  const htmlPt = renderServers(stateUnlisted, { t: tPt });
  assert.ok(
    htmlPt.includes('<option value="eth3" selected>eth3 (indisponível)</option>'),
    "Must render unavailable option selected when configured interface is not present in system interfaces"
  );

  // Case B: unlisted interface with English translation
  const tEn = createTranslator(en, ptBR);
  const htmlEn = renderServers(stateUnlisted, { t: tEn });
  assert.ok(
    htmlEn.includes('<option value="eth3" selected>eth3 (unavailable)</option>'),
    "Must render unavailable option in English"
  );

  // Case C: known interface should NOT render unavailable option
  const stateListed = createMockState({
    config: {
      provider: "cloudflare",
      interface: "eth0",
    },
    servers: {
      interfaces: [{ name: "eth0", addrs: ["192.168.1.10"] }],
      libreServers: [],
    },
  });
  const htmlListed = renderServers(stateListed, { t: tPt });
  assert.ok(!htmlListed.includes("indisponível"), "Must not render unavailable label when interface is present");

  // Case D: 'auto' interface should NOT render unavailable option
  const stateAuto = createMockState({
    config: {
      provider: "cloudflare",
      interface: "auto",
    },
    servers: {
      interfaces: [{ name: "eth0", addrs: ["192.168.1.10"] }],
      libreServers: [],
    },
  });
  const htmlAuto = renderServers(stateAuto, { t: tPt });
  assert.ok(!htmlAuto.includes("indisponível"), "Must not render unavailable label when interface is auto");
});

test("on_settings_submit_refreshes_status_and_persists_netrack_locale", async () => {
  const state = createMockState({
    status: { running: false, paused: false, nextRun: 1762520400000 },
    config: { cron: "0 * * * *", paused: false },
    localePreference: "pt-BR",
  });

  const mockStorage = new Map();
  const originalLocalStorage = globalThis.localStorage;
  globalThis.localStorage = {
    getItem: (k) => mockStorage.get(k) || null,
    setItem: (k, v) => mockStorage.set(k, String(v)),
    removeItem: (k) => mockStorage.delete(k),
  };

  try {
    let statusCalled = false;
    const mockClient = {
      async patchConfig() {
        return { ok: true };
      },
      async getStatus() {
        statusCalled = true;
        return { running: false, paused: true, nextRun: null };
      },
    };

    const fd = new FormData();
    fd.append("cron", "0 0 * * *");
    fd.append("paused", "true");
    fd.append("locale", "en");

    let localeCallbackFired = false;
    const result = await onSettingsSubmit(fd, state, mockClient, {
      t: tPt,
      onLocaleChange: async () => {
        localeCallbackFired = true;
      },
    });

    assert.equal(result.ok, true);
    assert.equal(statusCalled, true, "client.getStatus must be called on settings save success");
    assert.equal(state.status.paused, true, "state.status must be updated with refreshed status");
    assert.equal(state.status.nextRun, null);
    assert.equal(state.localePreference, "en");
    assert.equal(localeCallbackFired, true);
    assert.equal(mockStorage.get("netrack.locale"), "en", "Must write to netrack.locale key");
  } finally {
    if (originalLocalStorage !== undefined) {
      globalThis.localStorage = originalLocalStorage;
    } else {
      delete globalThis.localStorage;
    }
  }
});

test("load_view_data_recovers_config_for_servers_and_refreshes_status_for_overview", async () => {
  const state = createMockState({
    config: null,
    status: { running: false, paused: false, nextRun: null },
  });

  let configFetched = false;
  let statusFetched = false;

  const mockClient = {
    async getConfig() {
      configFetched = true;
      return { provider: "librespeed", interface: "auto", paused: true };
    },
    async getInterfaces() {
      return [{ name: "eth0", addrs: ["10.0.0.1"] }];
    },
    async getLibreServers() {
      return [];
    },
    async listSpeedtests() {
      return [];
    },
    async getStatistics() {
      return null;
    },
    async getStatus() {
      statusFetched = true;
      return { running: false, paused: true, nextRun: null };
    },
  };

  // 1. servers route lazy config recovery
  await loadViewData("servers", mockClient, state);
  assert.equal(configFetched, true, "getConfig must be called when state.config is null on servers route");
  assert.deepEqual(state.config, { provider: "librespeed", interface: "auto", paused: true });

  // 2. overview route status refresh
  await loadViewData("overview", mockClient, state);
  assert.equal(statusFetched, true, "getStatus must be called on overview route");
  assert.equal(state.status.paused, true, "state.status.paused must be refreshed");
});


test("settings_backup_keep_is_extracted_and_zero_is_valid", () => {
  for (const [raw, want] of [["0", 0], ["7", 7]]) {
    const fd = new FormData();
    fd.append("backupKeep", raw);
    assert.strictEqual(extractSettingsFormValues(fd).backupKeep, want);
  }
});

import test from "node:test";
import assert from "node:assert/strict";

import {
  ApiClientError,
  CONFIG_KEY_MAP,
  serializeConfigValue,
  createApiClient,
} from "../api.js";
import {
  createInitialState,
  applyRunEvent,
  parseRoute,
  shouldLoadNodes,
} from "../state.js";
import { setupI18n } from "../app.js";

test("apply_run_event_transitions_running_and_triggers_refresh", () => {
  const initial = createInitialState();
  assert.equal(initial.status.running, false);

  // 1. started event sets running: true, shouldRefresh: false
  const r1 = applyRunEvent(initial, { type: "started" });
  assert.equal(r1.nextState.status.running, true);
  assert.equal(r1.shouldRefresh, false);

  // 2. finished event sets running: false, shouldRefresh: true
  const r2 = applyRunEvent(r1.nextState, {
    type: "finished",
    id: 42,
    metrics: { download: 500, upload: 200, ping: 12 },
  });
  assert.equal(r2.nextState.status.running, false);
  assert.equal(r2.shouldRefresh, true);

  // 3. failed event sets running: false, populates banner, shouldRefresh: true
  const r3 = applyRunEvent(r1.nextState, {
    type: "failed",
    id: 43,
    error: "connection timeout",
  });
  assert.equal(r3.nextState.status.running, false);
  assert.equal(r3.shouldRefresh, true);
  assert.deepEqual(r3.nextState.banner, {
    kind: "error",
    text: "connection timeout",
  });

  // 4. expectedUpdated event updates config in place if config loaded
  const withConfig = {
    ...initial,
    config: {
      expectedPing: 20,
      expectedDownload: 300,
      expectedUpload: 150,
      provider: "cloudflare",
    },
  };
  const r4 = applyRunEvent(withConfig, {
    type: "expectedUpdated",
    ping: 15,
    download: 600,
    upload: 300,
  });
  assert.equal(r4.shouldRefresh, false);
  assert.equal(r4.nextState.config.expectedPing, 15);
  assert.equal(r4.nextState.config.expectedDownload, 600);
  assert.equal(r4.nextState.config.expectedUpload, 300);

  // If config is not yet loaded, expectedUpdated should not crash
  const r5 = applyRunEvent(initial, {
    type: "expectedUpdated",
    ping: 15,
    download: 600,
    upload: 300,
  });
  assert.equal(r5.shouldRefresh, false);
  assert.equal(r5.nextState.config, null);
});

test("serialize_config_value_and_key_map", () => {
  assert.equal(CONFIG_KEY_MAP.scheduleOffset, "schedule_offset");
  assert.equal(CONFIG_KEY_MAP.retentionDays, "retention_days");
  assert.equal(CONFIG_KEY_MAP.expectedMode, "expected_mode");
  assert.equal(CONFIG_KEY_MAP.expectedPing, "expected_ping");
  assert.equal(CONFIG_KEY_MAP.expectedDownload, "expected_download");
  assert.equal(CONFIG_KEY_MAP.expectedUpload, "expected_upload");
  assert.equal(CONFIG_KEY_MAP.libreServerId, "libre_server_id");
  assert.equal(CONFIG_KEY_MAP.libreUrl, "libre_url");
  assert.equal(CONFIG_KEY_MAP.interface, "interface");
  assert.equal(CONFIG_KEY_MAP.paused, "paused");
  assert.equal(CONFIG_KEY_MAP.provider, "provider");
  assert.equal(CONFIG_KEY_MAP.cron, "cron");

  assert.equal(serializeConfigValue("libreServerId", null), "auto");
  assert.equal(serializeConfigValue("libreServerId", ""), "auto");
  assert.equal(serializeConfigValue("interface", null), "auto");
  assert.equal(serializeConfigValue("interface", ""), "auto");
  assert.equal(serializeConfigValue("libreUrl", null), "none");
  assert.equal(serializeConfigValue("libreUrl", ""), "none");
  assert.equal(serializeConfigValue("scheduleOffset", true), "true");
  assert.equal(serializeConfigValue("scheduleOffset", false), "false");
  assert.equal(serializeConfigValue("retentionDays", 30), "30");
  assert.equal(serializeConfigValue("cron", "0 * * * *"), "0 * * * *");
});

test("api_client_parses_json_error_on_400_and_409", async () => {
  const mockFetch409 = async () => ({
    ok: false,
    status: 409,
    statusText: "Conflict",
    json: async () => ({ error: "test already running" }),
    headers: new Headers({ "content-type": "application/json" }),
  });

  const client = createApiClient(mockFetch409);
  await assert.rejects(
    async () => {
      await client.runSpeedtest();
    },
    (err) => {
      assert.ok(err instanceof ApiClientError);
      assert.equal(err.status, 409);
      assert.equal(err.message, "test already running");
      return true;
    }
  );

  const mockFetch400 = async () => ({
    ok: false,
    status: 400,
    statusText: "Bad Request",
    json: async () => ({ error: "invalid cron expression" }),
    headers: new Headers({ "content-type": "application/json" }),
  });

  const client400 = createApiClient(mockFetch400);
  await assert.rejects(
    async () => {
      await client400.patchConfig("cron", "invalid");
    },
    (err) => {
      assert.ok(err instanceof ApiClientError);
      assert.equal(err.status, 400);
      assert.equal(err.message, "invalid cron expression");
      return true;
    }
  );
});

test("create_initial_state_has_expected_shape", () => {
  const state = createInitialState();
  assert.equal(state.route, "overview");
  assert.equal(state.locale, "pt-BR");
  assert.deepEqual(state.status, { running: false, paused: false, nextRun: null });
  assert.deepEqual(state.system, {
    version: "—",
    uptimeSecs: 0,
    cpuPercent: null,
    rssBytes: null,
    tz: "UTC",
  });
  assert.equal(state.config, null);
  assert.deepEqual(state.recentTests, []);
  assert.equal(state.overviewStats, null);
  assert.deepEqual(state.statsRange, { preset: "7d", from: "", to: "" });
  assert.equal(state.statsData, null);
  assert.deepEqual(state.history, { rows: [], hasMore: false, loading: false });
  assert.deepEqual(state.servers, { interfaces: [], libreServers: [], libreError: null });
  assert.equal(state.banner, null);
});

test("parse_route_defaults_to_overview_and_matches_valid_routes", () => {
  assert.equal(parseRoute("/"), "overview");
  assert.equal(parseRoute(""), "overview");
  assert.equal(parseRoute(null), "overview");
  assert.equal(parseRoute("/overview"), "overview");
  assert.equal(parseRoute("/statistics"), "statistics");
  assert.equal(parseRoute("/history"), "history");
  assert.equal(parseRoute("/servers"), "servers");
  assert.equal(parseRoute("/nodes"), "nodes");
  assert.equal(parseRoute("/integrations"), "integrations");
  assert.equal(parseRoute("/settings"), "settings");
  assert.equal(parseRoute("/unknown-route"), "overview");
  assert.equal(parseRoute("/statistics?from=2026-01-01"), "statistics");
});

test("api_client_methods_dispatch_correct_urls_and_methods", async () => {
  const calls = [];
  const mockFetch = async (url, opts = {}) => {
    calls.push({ url, method: opts.method || "GET", body: opts.body, headers: opts.headers });
    return {
      ok: true,
      status: opts.method === "DELETE" ? 204 : 200,
      json: async () => ({ ok: true }),
      text: async () => JSON.stringify({ ok: true }),
      headers: new Headers({ "content-type": "application/json" }),
    };
  };

  const client = createApiClient(mockFetch);

  await client.getStatus();
  assert.equal(calls[calls.length - 1].url, "/api/speedtests/status");
  assert.equal(calls[calls.length - 1].method, "GET");

  await client.runSpeedtest();
  assert.equal(calls[calls.length - 1].url, "/api/speedtests/run");
  assert.equal(calls[calls.length - 1].method, "POST");

  await client.pause();
  assert.equal(calls[calls.length - 1].url, "/api/speedtests/pause");
  assert.equal(calls[calls.length - 1].method, "POST");

  await client.resume();
  assert.equal(calls[calls.length - 1].url, "/api/speedtests/continue");
  assert.equal(calls[calls.length - 1].method, "POST");

  await client.listSpeedtests({ before: 100, limit: 15 });
  assert.equal(calls[calls.length - 1].url, "/api/speedtests?before=100&limit=15");
  assert.equal(calls[calls.length - 1].method, "GET");

  await client.deleteSpeedtest(42);
  assert.equal(calls[calls.length - 1].url, "/api/speedtests/42");
  assert.equal(calls[calls.length - 1].method, "DELETE");

  await client.getStatistics("2026-03-01", "2026-03-07");
  assert.equal(calls[calls.length - 1].url, "/api/speedtests/statistics?from=2026-03-01&to=2026-03-07");
  assert.equal(calls[calls.length - 1].method, "GET");

  await client.getConfig();
  assert.equal(calls[calls.length - 1].url, "/api/config");
  assert.equal(calls[calls.length - 1].method, "GET");

  await client.patchConfig("scheduleOffset", true);
  assert.equal(calls[calls.length - 1].url, "/api/config/schedule_offset");
  assert.equal(calls[calls.length - 1].method, "PATCH");
  assert.deepEqual(JSON.parse(calls[calls.length - 1].body), { value: "true" });

  await client.patchConfig("libre_url", "");
  assert.equal(calls[calls.length - 1].url, "/api/config/libre_url");
  assert.equal(calls[calls.length - 1].method, "PATCH");
  assert.deepEqual(JSON.parse(calls[calls.length - 1].body), { value: "none" });

  await client.getSystemStatus();
  assert.equal(calls[calls.length - 1].url, "/api/system/status");
  assert.equal(calls[calls.length - 1].method, "GET");

  await client.getInterfaces();
  assert.equal(calls[calls.length - 1].url, "/api/system/interfaces");
  assert.equal(calls[calls.length - 1].method, "GET");

  await client.getLibreServers();
  assert.equal(calls[calls.length - 1].url, "/api/system/servers/librespeed");
  assert.equal(calls[calls.length - 1].method, "GET");

  await client.getLocales();
  assert.equal(calls[calls.length - 1].url, "/locales/index.json");
  assert.equal(calls[calls.length - 1].method, "GET");

  await client.getLocaleDict("pt-BR");
  assert.equal(calls[calls.length - 1].url, "/locales/pt-BR.json");
  assert.equal(calls[calls.length - 1].method, "GET");
});

test("setup_i18n_reads_netrack_locale_and_falls_back_to_legacy_key", async () => {
  const originalLocalStorage = globalThis.localStorage;
  const mockStorage = new Map();
  globalThis.localStorage = {
    getItem: (k) => mockStorage.get(k) || null,
    setItem: (k, v) => mockStorage.set(k, String(v)),
    removeItem: (k) => mockStorage.delete(k),
  };

  const mockClient = {
    async getLocales() {
      return [
        { code: "pt-BR", name: "Português (Brasil)" },
        { code: "en", name: "English" },
      ];
    },
    async getLocaleDict() {
      return {};
    },
  };

  try {
    // 1. Reading modern key "netrack.locale"
    mockStorage.set("netrack.locale", "en");
    mockStorage.delete("netrack_locale");
    const stateA = createInitialState();
    await setupI18n(mockClient, stateA);
    assert.equal(stateA.localePreference, "en");
    assert.equal(stateA.locale, "en");

    // 2. Fallback to legacy key "netrack_locale" when modern key is absent
    mockStorage.delete("netrack.locale");
    mockStorage.set("netrack_locale", "en");
    const stateB = createInitialState();
    await setupI18n(mockClient, stateB);
    assert.equal(stateB.localePreference, "en");
    assert.equal(stateB.locale, "en");

    // 3. When neither is present, defaults to "auto"
    mockStorage.delete("netrack.locale");
    mockStorage.delete("netrack_locale");
    const stateC = createInitialState();
    await setupI18n(mockClient, stateC);
    assert.equal(stateC.localePreference, "auto");
  } finally {
    if (originalLocalStorage !== undefined) {
      globalThis.localStorage = originalLocalStorage;
    } else {
      delete globalThis.localStorage;
    }
  }
});

test("shouldLoadNodes skips the node list only for anonymous visitors in public mode", () => {
  assert.equal(shouldLoadNodes({ mode: "public", authenticated: false }), false);
  assert.equal(shouldLoadNodes({ mode: "public", authenticated: true }), true);
  assert.equal(shouldLoadNodes({ mode: "none", authenticated: false }), true);
  assert.equal(shouldLoadNodes({ mode: "protected", authenticated: true }), true);
});

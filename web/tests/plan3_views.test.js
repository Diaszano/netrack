import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";

import { createTranslator } from "../i18n.js";
import { createApiClient, CONFIG_KEY_MAP, serializeConfigValue } from "../api.js";
import { createInitialState } from "../state.js";
import { renderAuthModal, renderAuthStatusBadge } from "../views/auth.js";
import { renderIntegrations, SUPPORTED_CHANNELS, SUPPORTED_EVENTS, CHANNEL_CONFIG_FIELDS } from "../views/integrations.js";
import { renderNodes } from "../views/nodes.js";
import { renderServers } from "../views/servers.js";
import { renderSettings } from "../views/settings.js";
import {
  extractServersFormValues,
  loadViewData,
  renderSystemBanners,
} from "../app.js";

const ptBR = JSON.parse(
  fs.readFileSync(new URL("../locales/pt-BR.json", import.meta.url), "utf8")
);
const en = JSON.parse(
  fs.readFileSync(new URL("../locales/en.json", import.meta.url), "utf8")
);
const tPt = createTranslator(ptBR, en);
const tEn = createTranslator(en, ptBR);

test("auth_view_renders_login_modal_and_status_badges", () => {
  const unauthedProtected = {
    auth: {
      mode: "protected",
      authenticated: false,
      modalOpen: false,
      error: "Invalid password",
    },
  };

  // Protected mode and not authenticated -> modal renders even if modalOpen is false
  const modalHtml = renderAuthModal(unauthedProtected, { t: tPt });
  assert.ok(modalHtml.includes('data-form="auth-login"'), "Must render login form");
  assert.ok(modalHtml.includes('name="password"'), "Must render password input");
  assert.ok(modalHtml.includes("Invalid password"), "Must display auth error");
  assert.ok(!modalHtml.includes('data-action="close-auth-modal"'), "Protected mode must not allow closing login modal");

  // Public mode with modalOpen: true -> renders login modal with close button
  const unauthedPublicModal = {
    auth: {
      mode: "public",
      authenticated: false,
      modalOpen: true,
      error: null,
    },
  };
  const pubModalHtml = renderAuthModal(unauthedPublicModal, { t: tPt });
  assert.ok(pubModalHtml.includes('data-form="auth-login"'), "Public mode with modalOpen must render login form");
  assert.ok(pubModalHtml.includes('data-action="close-auth-modal"'), "Public mode modal must have close button");

  // None mode or authenticated -> modal is empty
  const authedState = {
    auth: {
      mode: "protected",
      authenticated: true,
      modalOpen: false,
    },
  };
  assert.equal(renderAuthModal(authedState, { t: tPt }), "");

  // Topbar badge in public unauthenticated -> shows login button
  const badgeLogin = renderAuthStatusBadge(
    { auth: { mode: "public", authenticated: false } },
    { t: tPt }
  );
  assert.ok(badgeLogin.includes('data-action="open-auth-modal"'), "Public unauthenticated must show open-auth-modal button");

  // Topbar badge when authenticated -> shows logout button
  const badgeLogout = renderAuthStatusBadge(authedState, { t: tPt });
  assert.ok(badgeLogout.includes('data-action="auth-logout"'), "Authenticated state must show auth-logout button");
});

test("settings_view_renders_password_card_prometheus_and_data_management", () => {
  const state = createInitialState();
  state.config = {
    provider: "cloudflare",
    cron: "0 * * * *",
    scheduleOffset: true,
    retentionDays: 365,
    expectedMode: "auto",
    expectedDownload: 500,
    expectedUpload: 200,
    expectedPing: 15,
    passwordMode: "public",
  };
  state.auth = {
    mode: "public",
    authenticated: true,
  };
  state.dataStats = {
    rowCount: 142,
    dbSizeBytes: 5242880, // 5 MB
  };

  const html = renderSettings(state, {
    t: tPt,
    locales: [{ code: "pt-BR", name: "Português (Brasil)" }],
  });

  // 1. Password protection card
  assert.ok(html.includes('data-form="auth-password"'), "Must render password protection form");
  assert.ok(html.includes('name="passwordMode"') || html.includes('name="mode"'), "Must render password mode selector");
  assert.ok(html.includes('value="none"'), "Must include none mode option");
  assert.ok(html.includes('value="public"'), "Must include public mode option");
  assert.ok(html.includes('value="protected"'), "Must include protected mode option");

  // 2. Prometheus metrics info
  assert.ok(html.includes("/api/prometheus/metrics"), "Must show Prometheus metrics endpoint URL");

  // 3. Data Management card (Export JSON/CSV, Import Netrack/MySpeed, Stats & Reset)
  assert.ok(html.includes("/api/data/export?format=json") || html.includes('data-action="export-json"'), "Must include JSON export action/link");
  assert.ok(html.includes("/api/data/export?format=csv") || html.includes('data-action="export-csv"'), "Must include CSV export action/link");
  assert.ok(html.includes('data-form="data-import"') || html.includes('data-action="import-data"'), "Must include data import form/control");
  assert.ok(html.includes("142"), "Must display database row count (142)");
  assert.ok(html.includes("5 MB"), "Must display formatted database size (5 MB)");
  assert.ok(html.includes('data-action="reset-data"'), "Must include reset database button");
});

test("servers_view_renders_ookla_provider_consent_install_and_servers", () => {
  // Case 1: Ookla selected, not consented and not installed
  const stateUnconsented = createInitialState();
  stateUnconsented.config = {
    provider: "ookla",
    ooklaServerId: null,
    interface: "auto",
  };
  stateUnconsented.servers = {
    interfaces: [],
    libreServers: [],
    libreError: null,
    ooklaStatus: {
      installed: false,
      consented: false,
      arch: "aarch64",
    },
    ooklaServers: [],
    ooklaError: null,
    ooklaInstalling: false,
  };

  const html1 = renderServers(stateUnconsented, { t: tPt });
  assert.ok(html1.includes('value="ookla"'), "Must render ookla provider radio");
  assert.ok(html1.includes("Ookla"), "Must render Ookla label");
  assert.ok(html1.includes("EULA") || html1.includes("speedtest.net") || html1.includes("ookla"), "Must show EULA/consent info when not consented");
  assert.ok(
    html1.includes('data-action="ookla-consent-install"') || html1.includes('data-action="ookla-consent"'),
    "Must show consent & install action button"
  );

  // Case 2: Ookla consented & installed, server list loaded
  const stateInstalled = createInitialState();
  stateInstalled.config = {
    provider: "ookla",
    ooklaServerId: 12345,
    interface: "auto",
  };
  stateInstalled.servers = {
    interfaces: [],
    libreServers: [],
    libreError: null,
    ooklaStatus: {
      installed: true,
      consented: true,
      arch: "aarch64",
    },
    ooklaServers: [
      { id: 12345, name: "São Paulo Telecom", location: "São Paulo", country: "Brazil" },
      { id: 67890, name: "Rio Fibra", location: "Rio de Janeiro", country: "Brazil" },
    ],
    ooklaError: null,
    ooklaInstalling: false,
  };

  const html2 = renderServers(stateInstalled, { t: tPt });
  assert.ok(html2.includes('name="ooklaServerId"'), "Must render ooklaServerId select when installed");
  assert.ok(html2.includes("12345") && html2.includes("São Paulo Telecom"), "Must render Ookla server options");
  assert.ok(html2.includes('<option value="12345" selected>'), "Must mark configured Ookla server as selected");

  // Verify extractServersFormValues handles ooklaServerId
  const fd = new FormData();
  fd.append("provider", "ookla");
  fd.append("ooklaServerId", "67890");
  fd.append("interface", "auto");
  const extracted = extractServersFormValues(fd, stateInstalled.config);
  assert.equal(extracted.provider, "ookla");
  assert.equal(extracted.ooklaServerId, "67890");

  // Verify serializeConfigValue for ooklaServerId
  assert.equal(CONFIG_KEY_MAP.ooklaServerId, "ookla_server_id");
  assert.equal(serializeConfigValue("ooklaServerId", null), "auto");
  assert.equal(serializeConfigValue("ooklaServerId", ""), "auto");
  assert.equal(serializeConfigValue("ooklaServerId", "12345"), "12345");
});

test("integrations_view_renders_8_channels_6_events_and_crud_controls", () => {
  assert.equal(SUPPORTED_CHANNELS.length, 8);
  for (const ch of ["discord", "telegram", "pushover", "gotify", "ntfy", "webhook", "influxdb", "healthchecks"]) {
    assert.ok(SUPPORTED_CHANNELS.includes(ch), `Must support channel ${ch}`);
    assert.ok(Array.isArray(CHANNEL_CONFIG_FIELDS[ch]), `Must define config fields for ${ch}`);
  }
  assert.equal(SUPPORTED_EVENTS.length, 6);

  const state = createInitialState();
  state.integrations = {
    items: [
      {
        id: 1,
        name: "My Discord Alert",
        channel_type: "discord",
        config: { webhook_url: "https://discord.com/api/webhooks/123" },
        events: ["speedtest.finished", "speedtest.failed"],
        enabled: true,
      },
      {
        id: 2,
        name: "Telegram Bot",
        channel_type: "telegram",
        config: { bot_token: "123:ABC", chat_id: "999" },
        events: ["speedtest.failed"],
        enabled: false,
      },
    ],
    editing: null,
    selectedChannel: "discord",
  };

  const html = renderIntegrations(state, { t: tPt });

  // Renders existing integrations with test, edit, delete buttons
  assert.ok(html.includes("My Discord Alert"), "Must render integration name");
  assert.ok(html.includes("Telegram Bot"), "Must render second integration name");
  assert.ok(html.includes('data-action="test-integration" data-id="1"'), "Must render test button for integration 1");
  assert.ok(html.includes('data-action="edit-integration" data-id="1"'), "Must render edit button for integration 1");
  assert.ok(html.includes('data-action="delete-integration" data-id="1"'), "Must render delete button for integration 1");

  // Renders form to add/edit integration with channel selector and event checkboxes
  assert.ok(html.includes('data-form="integration"'), "Must render integration form");
  assert.ok(html.includes('name="channel_type"'), "Must render channel_type select");
  for (const ch of SUPPORTED_CHANNELS) {
    assert.ok(html.includes(`value="${ch}"`), `Form must include option for ${ch}`);
  }
  assert.ok(html.includes("webhook_url"), "Must render dynamic config field for selected channel (discord -> webhook_url)");
});

test("nodes_view_renders_remote_nodes_metrics_and_switcher", () => {
  const state = createInitialState();
  state.system = {
    version: "1.0.0",
    uptimeSecs: 3600,
    cpuPercent: 8.2,
    rssBytes: 31457280,
    tz: "America/Sao_Paulo",
  };
  state.nodes = {
    activeNodeId: null,
    items: [
      {
        id: 10,
        name: "Branch Office SP",
        url: "http://10.0.1.50:8080",
        hasPassword: true,
      },
      {
        id: 20,
        name: "Backup Link RJ",
        url: "http://10.0.2.50:8080",
        hasPassword: false,
      },
    ],
    statuses: {
      10: {
        online: true,
        version: "1.0.0",
        latestTest: {
          download: 480.5,
          upload: 210.2,
          ping: 9.4,
        },
        error: null,
      },
      20: {
        online: false,
        version: null,
        latestTest: null,
        error: "Connection refused",
      },
    },
    editing: null,
  };

  const html = renderNodes(state, { t: tPt });

  // Local server card
  assert.ok(html.includes("1.0.0"), "Must render local server version");

  // Remote nodes list
  assert.ok(html.includes("Branch Office SP"), "Must render online remote node name");
  assert.ok(html.includes("http://10.0.1.50:8080"), "Must render remote node URL");
  assert.ok(html.includes("480") || html.includes("481"), "Must render latest download metric on node card");
  assert.ok(html.includes("Backup Link RJ"), "Must render offline remote node name");
  assert.ok(html.includes("Connection refused"), "Must render error on offline node card");

  // Node actions: switch context, edit, delete
  assert.ok(html.includes('data-action="switch-node" data-id="10"'), "Must render switch-node button for remote node");
  assert.ok(html.includes('data-action="edit-node" data-id="10"'), "Must render edit-node button");
  assert.ok(html.includes('data-action="delete-node" data-id="10"'), "Must render delete-node button");
  assert.ok(html.includes('data-form="node"'), "Must render add/edit node form");
});

test("api_client_supports_node_proxy_routing_and_new_endpoints", async () => {
  const calls = [];
  const mockFetch = async (url, opts = {}) => {
    calls.push({ url, method: opts.method || "GET", body: opts.body });
    return {
      ok: true,
      status: opts.method === "DELETE" ? 204 : 200,
      json: async () => ({ ok: true }),
      text: async () => JSON.stringify({ ok: true }),
      headers: new Headers({ "content-type": "application/json" }),
    };
  };

  const client = createApiClient(mockFetch);

  // Local call
  await client.getStatus();
  assert.equal(calls[calls.length - 1].url, "/api/speedtests/status");

  // Switch active node context to node 10
  client.setActiveNode(10);
  assert.equal(client.getActiveNode(), 10);

  // Proxied call routes through /api/nodes/10/proxy/...
  await client.getStatus();
  assert.equal(calls[calls.length - 1].url, "/api/nodes/10/proxy/speedtests/status");

  await client.listSpeedtests({ limit: 10 });
  assert.equal(calls[calls.length - 1].url, "/api/nodes/10/proxy/speedtests?limit=10");

  // Node management endpoints themselves always stay on local instance
  await client.listNodes();
  assert.equal(calls[calls.length - 1].url, "/api/nodes");

  // Switch back to local node
  client.setActiveNode(null);
  await client.getStatus();
  assert.equal(calls[calls.length - 1].url, "/api/speedtests/status");

  // Auth endpoints
  await client.getAuthStatus();
  assert.equal(calls[calls.length - 1].url, "/api/auth/status");

  await client.login("secret");
  assert.equal(calls[calls.length - 1].url, "/api/auth/login");
  assert.equal(calls[calls.length - 1].method, "POST");

  await client.logout();
  assert.equal(calls[calls.length - 1].url, "/api/auth/logout");
  assert.equal(calls[calls.length - 1].method, "POST");

  await client.setPassword({ password: "new", mode: "protected" });
  assert.equal(calls[calls.length - 1].url, "/api/auth/password");

  // Ookla endpoints
  await client.getOoklaStatus();
  assert.equal(calls[calls.length - 1].url, "/api/system/ookla/status");

  await client.consentOokla();
  assert.equal(calls[calls.length - 1].url, "/api/system/ookla/consent");
  assert.equal(calls[calls.length - 1].method, "POST");

  await client.installOokla();
  assert.equal(calls[calls.length - 1].url, "/api/system/ookla/install");
  assert.equal(calls[calls.length - 1].method, "POST");

  await client.getOoklaServers();
  assert.equal(calls[calls.length - 1].url, "/api/system/servers/ookla");

  // Integrations endpoints
  await client.listIntegrations();
  assert.equal(calls[calls.length - 1].url, "/api/integrations");

  await client.createIntegration({ name: "Hook", channel_type: "webhook", config: { url: "http://x" }, events: [] });
  assert.equal(calls[calls.length - 1].url, "/api/integrations");
  assert.equal(calls[calls.length - 1].method, "POST");

  await client.updateIntegration(5, { name: "Hook2", channel_type: "webhook", config: { url: "http://x" }, events: [] });
  assert.equal(calls[calls.length - 1].url, "/api/integrations/5");
  assert.equal(calls[calls.length - 1].method, "PUT");

  await client.deleteIntegration(5);
  assert.equal(calls[calls.length - 1].url, "/api/integrations/5");
  assert.equal(calls[calls.length - 1].method, "DELETE");

  await client.testIntegration(5);
  assert.equal(calls[calls.length - 1].url, "/api/integrations/5/test");
  assert.equal(calls[calls.length - 1].method, "POST");

  // Data endpoints
  await client.getDataStats();
  assert.equal(calls[calls.length - 1].url, "/api/data/stats");

  await client.importData('[{"ping":10}]');
  assert.equal(calls[calls.length - 1].url, "/api/data/import");
  assert.equal(calls[calls.length - 1].method, "POST");

  await client.resetData();
  assert.equal(calls[calls.length - 1].url, "/api/data/reset");
});

test("system_banners_render_preview_mode_and_update_available", () => {
  const state = createInitialState();
  state.system = {
    version: "0.1.0",
    latest_version: "0.2.0",
    update_available: true,
    preview_mode: true,
    preview_message: "Modo de demonstração ativo",
    uptimeSecs: 100,
    cpuPercent: 5,
    rssBytes: 10000000,
    tz: "UTC",
  };
  state.banner = { kind: "error", text: "Custom error alert" };

  const bannersHtml = renderSystemBanners(state, { t: tPt });
  assert.ok(bannersHtml.includes("Modo de demonstração ativo"), "Must render preview_message banner");
  assert.ok(bannersHtml.includes("0.2.0"), "Must render update_available notification with latest_version");
  assert.ok(bannersHtml.includes("Custom error alert"), "Must also render active state.banner alert");
});

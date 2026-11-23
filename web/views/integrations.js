// web/views/integrations.js
// Notification & Integration channels CRUD view (Discord, Telegram, Pushover, Gotify, ntfy, Webhook, InfluxDB, Healthchecks)

import { escapeHtml } from "../i18n.js";

export const SUPPORTED_CHANNELS = [
  "discord",
  "telegram",
  "pushover",
  "gotify",
  "ntfy",
  "webhook",
  "influxdb",
  "healthchecks",
];

export const SUPPORTED_EVENTS = [
  "speedtest.started",
  "speedtest.finished",
  "speedtest.failed",
  "speedtest.degraded",
  "speedtest.recovered",
  "minute.passed",
  "config.updated",
  "expected.updated",
];

export const EVENT_ALIASES = {
  test_started: "speedtest.started",
  test_completed: "speedtest.finished",
  test_failed: "speedtest.failed",
  ping_exceeded: "expected.updated",
  download_below: "config.updated",
  upload_below: "minute.passed",
};

export const CHANNEL_CONFIG_FIELDS = {
  discord: [
    { key: "webhook_url", labelKey: "integrations.fieldWebhookUrl", defaultLabel: "Webhook URL", type: "url", required: true, placeholder: "https://discord.com/api/webhooks/..." },
    { key: "username", labelKey: "integrations.fieldUsername", defaultLabel: "Username (opcional)", type: "text", required: false, placeholder: "Netrack" },
    { key: "avatar_url", labelKey: "integrations.fieldAvatarUrl", defaultLabel: "Avatar URL (opcional)", type: "url", required: false, placeholder: "https://..." },
  ],
  telegram: [
    { key: "bot_token", labelKey: "integrations.fieldBotToken", defaultLabel: "Bot Token", type: "password", required: true, placeholder: "123456:ABC-DEF..." },
    { key: "chat_id", labelKey: "integrations.fieldChatId", defaultLabel: "Chat ID", type: "text", required: true, placeholder: "-1001234567890" },
    { key: "api_url", labelKey: "integrations.fieldApiUrl", defaultLabel: "API URL (opcional)", type: "url", required: false, placeholder: "https://api.telegram.org" },
  ],
  pushover: [
    { key: "user_key", labelKey: "integrations.fieldUserKey", defaultLabel: "User Key", type: "text", required: true, placeholder: "uQiRzpo4DXghDmr9QzzfQu27cmVRsG" },
    { key: "api_token", labelKey: "integrations.fieldApiToken", defaultLabel: "API Token", type: "password", required: true, placeholder: "azGDORePK8gMaC0QOYAMyEEuzJnyUi" },
    { key: "device", labelKey: "integrations.fieldDevice", defaultLabel: "Dispositivo (opcional)", type: "text", required: false, placeholder: "iphone" },
    { key: "priority", labelKey: "integrations.fieldPriority", defaultLabel: "Prioridade (-2 a 2)", type: "number", required: false, placeholder: "0" },
  ],
  gotify: [
    { key: "server_url", labelKey: "integrations.fieldServerUrl", defaultLabel: "Server URL", type: "url", required: true, placeholder: "https://gotify.example.com" },
    { key: "app_token", labelKey: "integrations.fieldAppToken", defaultLabel: "App Token", type: "password", required: true, placeholder: "A1b2C3d4..." },
    { key: "priority", labelKey: "integrations.fieldPriority", defaultLabel: "Prioridade", type: "number", required: false, placeholder: "5" },
  ],
  ntfy: [
    { key: "topic", labelKey: "integrations.fieldTopic", defaultLabel: "Tópico", type: "text", required: true, placeholder: "netrack-alerts" },
    { key: "server_url", labelKey: "integrations.fieldServerUrl", defaultLabel: "Server URL (opcional)", type: "url", required: false, placeholder: "https://ntfy.sh" },
    { key: "auth_token", labelKey: "integrations.fieldAuthToken", defaultLabel: "Auth Token (opcional)", type: "password", required: false, placeholder: "tk_..." },
  ],
  webhook: [
    { key: "url", labelKey: "integrations.fieldUrl", defaultLabel: "Webhook URL", type: "url", required: true, placeholder: "https://example.com/webhook" },
    { key: "secret", labelKey: "integrations.fieldSecret", defaultLabel: "Secret Header (opcional)", type: "password", required: false, placeholder: "x-webhook-secret" },
  ],
  influxdb: [
    { key: "url", labelKey: "integrations.fieldUrl", defaultLabel: "InfluxDB URL", type: "url", required: true, placeholder: "http://localhost:8086" },
    { key: "version", labelKey: "integrations.fieldVersion", defaultLabel: "Versão (v2 ou v1)", type: "text", required: false, placeholder: "v2" },
    { key: "org", labelKey: "integrations.fieldOrg", defaultLabel: "Organização", type: "text", required: false, placeholder: "netrack" },
    { key: "bucket", labelKey: "integrations.fieldBucket", defaultLabel: "Bucket / Database", type: "text", required: false, placeholder: "netrack" },
    { key: "token", labelKey: "integrations.fieldToken", defaultLabel: "Token", type: "password", required: false, placeholder: "my-token" },
    { key: "measurement", labelKey: "integrations.fieldMeasurement", defaultLabel: "Measurement", type: "text", required: false, placeholder: "netrack" },
  ],
  healthchecks: [
    { key: "ping_url", labelKey: "integrations.fieldPingUrl", defaultLabel: "Ping URL", type: "url", required: true, placeholder: "https://hc-ping.com/your-uuid" },
  ],
};

const CHANNEL_LABELS = {
  discord: "Discord",
  telegram: "Telegram",
  pushover: "Pushover",
  gotify: "Gotify",
  ntfy: "ntfy",
  webhook: "Webhook",
  influxdb: "InfluxDB",
  healthchecks: "Healthchecks",
};

const EVENT_LABEL_MAP = {
  "speedtest.started": { key: "integrations.eventStarted", fallback: "Teste iniciado (speedtest.started)" },
  "speedtest.finished": { key: "integrations.eventFinished", fallback: "Teste concluído (speedtest.finished)" },
  "speedtest.failed": { key: "integrations.eventFailed", fallback: "Falha no teste (speedtest.failed)" },
  "speedtest.degraded": { key: "integrations.eventDegraded", fallback: "Teste lento (speedtest.degraded)" },
  "speedtest.recovered": { key: "integrations.eventRecovered", fallback: "Velocidade normalizada (speedtest.recovered)" },
  "expected.updated": { key: "integrations.eventExpectedUpdated", fallback: "Plano/limites atualizados (expected.updated)" },
  "config.updated": { key: "integrations.eventConfigUpdated", fallback: "Configuração alterada (config.updated)" },
  "minute.passed": { key: "integrations.eventHeartbeat", fallback: "Heartbeat por minuto (minute.passed)" },
};

const SECRET_MASK = "***";

/**
 * Extracts integration payload from FormData for POST/PUT /api/integrations.
 */
export function extractIntegrationPayload(formData) {
  const name = (formData.get("name") || "").trim();
  const channel_type = formData.get("channel_type") || "discord";
  const enabled = formData.has("enabled");
  const fields = CHANNEL_CONFIG_FIELDS[channel_type] || [];

  const config = {};
  for (const field of fields) {
    const raw = formData.get(`config.${field.key}`);
    if (raw != null && String(raw).trim() !== "") {
      const strVal = String(raw).trim();
      config[field.key] = field.type === "number" && !isNaN(Number(strVal)) ? Number(strVal) : strVal;
    } else if (formData.get(`masked.${field.key}`)) {
      // Untouched masked secret: send the mask back so the server keeps the stored value.
      config[field.key] = SECRET_MASK;
    }
  }

  const rawEvents = formData.getAll("events");
  const events = rawEvents
    .map((ev) => EVENT_ALIASES[ev] || ev)
    .filter((ev, idx, arr) => SUPPORTED_EVENTS.includes(ev) && arr.indexOf(ev) === idx);

  return {
    name,
    channel_type,
    config,
    events,
    enabled,
  };
}

/**
 * Renders dynamic configuration inputs for a specific channel type.
 */
export function renderChannelConfigFields(channelType, currentConfig = {}, { t = (k) => k } = {}) {
  const fields = CHANNEL_CONFIG_FIELDS[channelType] || CHANNEL_CONFIG_FIELDS.discord;
  return fields
    .map((field) => {
      const label = t(field.labelKey) !== field.labelKey ? t(field.labelKey) : field.defaultLabel;
      const val = currentConfig && currentConfig[field.key] != null ? String(currentConfig[field.key]) : "";
      const isMasked = val === SECRET_MASK;
      const reqAttr = field.required && !isMasked ? " required" : "";
      const type = isMasked ? "password" : field.type;
      const placeholder = isMasked ? "••••" : field.placeholder || "";
      const maskedInput = isMasked ? `<input type="hidden" name="masked.${escapeHtml(field.key)}" value="1">` : "";
      return `<div class="form-group">
      <label class="form-label" for="int-cfg-${escapeHtml(field.key)}">${escapeHtml(label)}</label>
      <input id="int-cfg-${escapeHtml(field.key)}" type="${escapeHtml(type)}" name="config.${escapeHtml(field.key)}" value="${isMasked ? "" : escapeHtml(val)}" placeholder="${escapeHtml(placeholder)}" class="form-control"${reqAttr}>
      ${maskedInput}
    </div>`;
    })
    .join("\n");
}

/**
 * Renders the Integrations CRUD view.
 *
 * @param {object} state - Application state
 * @param {object} [opts]
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t] - Translator function
 * @returns {string} HTML string
 */
export function renderIntegrations(state, { t = (k) => k } = {}) {
  const intState = state?.integrations || {};
  const items = Array.isArray(intState.items) ? intState.items : [];
  const editing = intState.editing || null;
  const selectedChannel = editing?.channel_type || intState.selectedChannel || "discord";
  const currentConfig = editing?.config || {};
  const currentEvents = Array.isArray(editing?.events)
    ? editing.events
    : ["speedtest.finished", "speedtest.failed"];
  const isEnabled = editing ? Boolean(editing.enabled) : true;

  const title = t("integrations.title") || "Integrações";
  const headerHtml = `<div class="view-header">
  <h1 class="view-title">${escapeHtml(title)}</h1>
</div>`;

  // 1. Existing integrations list card
  let listContentHtml = "";
  if (items.length === 0) {
    listContentHtml = `<div class="card empty-state">
      <div class="empty-state-title">${escapeHtml(t("integrations.emptyTitle") || "Nenhuma integração configurada")}</div>
      <div class="empty-state-desc">${escapeHtml(t("integrations.emptyDescription") || "Adicione canais abaixo para receber notificações no Discord, Telegram, Pushover, Gotify, ntfy, Webhook, InfluxDB ou Healthchecks.")}</div>
    </div>`;
  } else {
    const rowsHtml = items
      .map((item) => {
        const chLabel = CHANNEL_LABELS[item.channel_type] || item.channel_type;
        const statusBadge = item.enabled
          ? `<span class="badge badge-ok">${escapeHtml(t("integrations.enabled") || "ativo")}</span>`
          : `<span class="badge badge-manual">${escapeHtml(t("integrations.disabled") || "inativo")}</span>`;
        const eventsCount = Array.isArray(item.events) ? item.events.length : 0;

        return `<tr>
          <td><strong>${escapeHtml(item.name)}</strong></td>
          <td><span class="badge badge-auto">${escapeHtml(chLabel)}</span></td>
          <td class="num-col">${escapeHtml(eventsCount)}</td>
          <td>${statusBadge}</td>
          <td style="text-align: right;">
            <div style="display: inline-flex; gap: 6px;">
              <button type="button" class="btn btn-secondary btn-sm" data-action="test-integration" data-id="${escapeHtml(item.id)}">${escapeHtml(t("integrations.test") || "Testar")}</button>
              <button type="button" class="btn btn-secondary btn-sm" data-action="edit-integration" data-id="${escapeHtml(item.id)}">${escapeHtml(t("common.edit") || "Editar")}</button>
              <button type="button" class="btn btn-danger" data-action="delete-integration" data-id="${escapeHtml(item.id)}">${escapeHtml(t("common.delete") || "Excluir")}</button>
            </div>
          </td>
        </tr>`;
      })
      .join("\n");

    listContentHtml = `<div class="card">
      <div class="card-header">
        <h2 class="card-title">${escapeHtml(t("integrations.configuredList") || "Canais configurados")}</h2>
      </div>
      <div class="table-container">
        <table class="dense-table">
          <thead>
            <tr>
              <th>${escapeHtml(t("integrations.colName") || "Nome")}</th>
              <th>${escapeHtml(t("integrations.colChannel") || "Canal")}</th>
              <th>${escapeHtml(t("integrations.colEvents") || "Eventos")}</th>
              <th>${escapeHtml(t("history.colStatus") || "Status")}</th>
              <th style="text-align: right;">${escapeHtml(t("history.colActions") || "Ações")}</th>
            </tr>
          </thead>
          <tbody>
            ${rowsHtml}
          </tbody>
        </table>
      </div>
    </div>`;
  }

  // 2. Add / Edit Integration Form card
  const formTitle = editing
    ? (t("integrations.editTitle") || "Editar integração")
    : (t("integrations.addTitle") || "Nova integração");

  const channelOptionsHtml = SUPPORTED_CHANNELS.map((ch) => {
    const isSel = ch === selectedChannel;
    const label = CHANNEL_LABELS[ch] || ch;
    return `<option value="${escapeHtml(ch)}"${isSel ? " selected" : ""}>${escapeHtml(label)}</option>`;
  }).join("\n");

  const configFieldsHtml = renderChannelConfigFields(selectedChannel, currentConfig, { t });

  const eventsCheckboxesHtml = SUPPORTED_EVENTS.map((ev) => {
    const info = EVENT_LABEL_MAP[ev] || { key: ev, fallback: ev };
    const label = t(info.key) !== info.key ? t(info.key) : info.fallback;
    const checked = currentEvents.includes(ev);
    return `<label class="form-check">
      <input type="checkbox" name="events" value="${escapeHtml(ev)}"${checked ? " checked" : ""}>
      <span>${escapeHtml(label)}</span>
    </label>`;
  }).join("\n");

  const cancelEditBtnHtml = editing
    ? `<button type="button" class="btn btn-secondary" data-action="cancel-edit-integration">${escapeHtml(t("common.cancel") || "Cancelar")}</button>`
    : "";

  const formCardHtml = `<div class="card">
    <div class="card-header">
      <h2 class="card-title">${escapeHtml(formTitle)}</h2>
    </div>
    <form data-form="integration" data-edit-id="${escapeHtml(editing?.id ?? "")}">
      <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 14px; margin-bottom: 16px;">
        <div class="form-group" style="margin-bottom: 0;">
          <label class="form-label" for="int-name">${escapeHtml(t("integrations.colName") || "Nome")}</label>
          <input id="int-name" type="text" name="name" value="${escapeHtml(editing?.name || "")}" class="form-control" required placeholder="Alerta Discord">
        </div>
        <div class="form-group" style="margin-bottom: 0;">
          <label class="form-label" for="int-channel">${escapeHtml(t("integrations.colChannel") || "Canal")}</label>
          <select id="int-channel" name="channel_type" class="form-control">
            ${channelOptionsHtml}
          </select>
        </div>
      </div>

      <div id="integration-config-fields" style="margin-bottom: 16px;">
        ${configFieldsHtml}
      </div>

      <div class="form-group">
        <label class="form-label">${escapeHtml(t("integrations.eventsTitle") || "Eventos disparados")}</label>
        <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 8px; margin-top: 4px;">
          ${eventsCheckboxesHtml}
        </div>
      </div>

      <div class="form-group">
        <label class="form-check">
          <input type="checkbox" name="enabled" value="true"${isEnabled ? " checked" : ""}>
          <span>${escapeHtml(t("integrations.enableChannel") || "Canal habilitado")}</span>
        </label>
      </div>

      <div style="display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px;">
        ${cancelEditBtnHtml}
        <button type="submit" class="btn btn-primary">${escapeHtml(t("common.save") || "Salvar alterações")}</button>
      </div>
    </form>
  </div>`;

  return `${headerHtml}
${listContentHtml}
${formCardHtml}`;
}

// web/views/settings.js
// Schedule, expected performance plan, language, and config diff persistence

import { escapeHtml } from "../i18n.js";
import { CONFIG_KEY_MAP } from "../api.js";

const SNAKE_TO_CAMEL = {};
for (const [camel, snake] of Object.entries(CONFIG_KEY_MAP)) {
  SNAKE_TO_CAMEL[snake] = camel;
}

/**
 * Checks whether a configuration field's value has changed between previous and new states.
 */
function isValueChanged(prevVal, nextVal) {
  if (prevVal === nextVal) {
    return false;
  }

  // Treat null, undefined and empty string as equivalent for unconfigured fields
  if ((prevVal == null || prevVal === "") && (nextVal == null || nextVal === "")) {
    return false;
  }

  // Boolean normalization
  if (typeof prevVal === "boolean") {
    const normalizedNext = nextVal === true || nextVal === "true";
    return prevVal !== normalizedNext;
  }

  // Number normalization
  if (typeof prevVal === "number" && nextVal !== "" && nextVal != null && !isNaN(Number(nextVal))) {
    return prevVal !== Number(nextVal);
  }

  return true;
}

/**
 * Diffs changed fields and calls `apiClient.patchConfig(field, value)` sequentially.
 * Catches errors and returns `{ ok: false, error: err.message }` without throwing.
 *
 * @param {object} prevConfig - Current config object in state
 * @param {Record<string, unknown>} formValues - Form values to persist
 * @param {object} apiClient - Netrack API client instance
 * @returns {Promise<{ ok: boolean, error?: string }>}
 */
export async function saveConfigDiff(prevConfig, formValues, apiClient) {
  if (!formValues || typeof formValues !== "object") {
    return { ok: true };
  }

  const entries = Object.entries(formValues);

  for (const [key, nextVal] of entries) {
    // Only process known configuration keys
    if (!(key in CONFIG_KEY_MAP) && !(key in SNAKE_TO_CAMEL)) {
      continue;
    }

    const camelKey = CONFIG_KEY_MAP[key] ? key : SNAKE_TO_CAMEL[key];
    const prevVal = prevConfig ? (prevConfig[camelKey] ?? prevConfig[key]) : undefined;

    if (isValueChanged(prevVal, nextVal)) {
      try {
        await apiClient.patchConfig(key, nextVal);
      } catch (err) {
        return { ok: false, error: err.message || String(err) };
      }
    }
  }

  return { ok: true };
}

/**
 * Renders the Settings view with Schedule, Expected Plan, and Language cards.
 *
 * @param {object} state - Application state
 * @param {object} [opts]
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t] - Translator function
 * @param {Array<{ code: string, name: string }>} [opts.locales] - Available languages list
 * @returns {string} HTML string
 */
export function renderSettings(state, { t = (k) => k, locales = [] } = {}) {
  const config = state?.config || {};
  const currentLocale = state?.localePreference || state?.locale || "auto";

  const title = t("settings.title") || "Ajustes";

  // Header
  const headerHtml = `<div class="view-header">
  <h1 class="view-title">${escapeHtml(title)}</h1>
</div>`;

  // 1. Agendamento card (cron, scheduleOffset, paused, retentionDays)
  const cronVal = config.cron ?? "0 * * * *";
  const offsetChecked = Boolean(config.scheduleOffset);
  const pausedChecked = Boolean(config.paused ?? state?.status?.paused);
  const retentionVal = config.retentionDays ?? 365;

  const scheduleCardHtml = `<div class="card">
  <div class="card-header">
    <h2 class="card-title">${escapeHtml(t("settings.schedule") || "Agendamento")}</h2>
  </div>
  <div class="form-group">
    <label class="form-label" for="setting-cron">${escapeHtml(t("settings.cron") || "Expressão Cron")}</label>
    <input id="setting-cron" type="text" name="cron" value="${escapeHtml(cronVal)}" class="form-control" placeholder="0 * * * *">
    <span class="form-help">${escapeHtml(t("settings.cronHelp") || "Define o intervalo de execução (ex: 0 * * * * para rodar a cada hora)")}</span>
  </div>
  <div class="form-group">
    <label class="form-check">
      <input type="checkbox" name="scheduleOffset" value="true"${offsetChecked ? " checked" : ""}>
      <span>${escapeHtml(t("settings.scheduleOffset") || "Deslocamento aleatório (offset)")}</span>
    </label>
    <span class="form-help" style="margin-left: 24px;">${escapeHtml(t("settings.scheduleOffsetHelp") || "Adiciona um pequeno atraso aleatório para evitar picos de tráfego")}</span>
  </div>
  <div class="form-group">
    <label class="form-check">
      <input type="checkbox" name="paused" value="true"${pausedChecked ? " checked" : ""}>
      <span>${escapeHtml(t("settings.pauseTests") || "Pausar agendamento de testes")}</span>
    </label>
  </div>
  <div class="form-group">
    <label class="form-label" for="setting-retention">${escapeHtml(t("settings.retentionDays") || "Retenção de dados (dias)")}</label>
    <input id="setting-retention" type="number" name="retentionDays" min="1" max="10000" value="${escapeHtml(retentionVal)}" class="form-control" style="max-width: 200px;">
    <span class="form-help">${escapeHtml(t("settings.retentionHelp") || "Testes com mais tempo que este período serão excluídos automaticamente")}</span>
  </div>
</div>`;

  // 2. Plano / Valores esperados card (expectedMode, expectedDownload, expectedUpload, expectedPing)
  const mode = config.expectedMode || "auto";
  const isAuto = mode === "auto";
  const dlVal = config.expectedDownload != null ? config.expectedDownload : "";
  const ulVal = config.expectedUpload != null ? config.expectedUpload : "";
  const pingVal = config.expectedPing != null ? config.expectedPing : "";

  const expectedCardHtml = `<div class="card">
  <div class="card-header">
    <h2 class="card-title">${escapeHtml(t("settings.plan") || "Plano contratado")}</h2>
  </div>
  <div class="form-group">
    <label class="form-label" for="setting-expected-mode">${escapeHtml(t("settings.expectedMode") || "Modo de valores esperados")}</label>
    <select id="setting-expected-mode" name="expectedMode" class="form-control">
      <option value="auto"${isAuto ? " selected" : ""}>${escapeHtml(t("settings.modeAuto") || "Automático (calculado com base nos melhores testes)")}</option>
      <option value="manual"${!isAuto ? " selected" : ""}>${escapeHtml(t("settings.modeManual") || "Manual")}</option>
    </select>
  </div>
  <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 14px;">
    <div class="form-group">
      <label class="form-label" for="setting-expected-dl">${escapeHtml(t("settings.expectedDownload") || "Download esperado (Mbps)")}</label>
      <input id="setting-expected-dl" type="number" step="any" min="0" name="expectedDownload" value="${escapeHtml(dlVal)}" class="form-control"${isAuto ? " readonly" : ""}>
    </div>
    <div class="form-group">
      <label class="form-label" for="setting-expected-ul">${escapeHtml(t("settings.expectedUpload") || "Upload esperado (Mbps)")}</label>
      <input id="setting-expected-ul" type="number" step="any" min="0" name="expectedUpload" value="${escapeHtml(ulVal)}" class="form-control"${isAuto ? " readonly" : ""}>
    </div>
    <div class="form-group">
      <label class="form-label" for="setting-expected-ping">${escapeHtml(t("settings.expectedPing") || "Ping esperado (ms)")}</label>
      <input id="setting-expected-ping" type="number" step="any" min="0" name="expectedPing" value="${escapeHtml(pingVal)}" class="form-control"${isAuto ? " readonly" : ""}>
    </div>
  </div>
</div>`;

  // 3. Idioma card (<select name="locale"> with auto + locales entries)
  let localeOptionsHtml = `<option value="auto"${currentLocale === "auto" ? " selected" : ""}>${escapeHtml(t("settings.languageAuto") || "Automático (preferência do navegador)")}</option>`;
  for (const loc of locales) {
    const isSel = currentLocale === loc.code;
    localeOptionsHtml += `\n<option value="${escapeHtml(loc.code)}"${isSel ? " selected" : ""}>${escapeHtml(loc.name)}</option>`;
  }

  const languageCardHtml = `<div class="card">
  <div class="card-header">
    <h2 class="card-title">${escapeHtml(t("settings.language") || "Idioma")}</h2>
  </div>
  <div class="form-group">
    <select name="locale" class="form-control" style="max-width: 300px;">
      ${localeOptionsHtml}
    </select>
  </div>
</div>`;

  // Submit button
  const submitBtnHtml = `<div style="display: flex; justify-content: flex-end; margin-top: 16px;">
  <button type="submit" class="btn btn-primary">${escapeHtml(t("common.save") || "Salvar alterações")}</button>
</div>`;

  return `${headerHtml}
<form data-form="settings">
  ${scheduleCardHtml}
  ${expectedCardHtml}
  ${languageCardHtml}
  ${submitBtnHtml}
</form>`;
}

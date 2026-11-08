// web/views/settings.js
// Schedule, expected performance plan, language, password protection, Prometheus, and data management view

import { escapeHtml } from "../i18n.js";
import { CONFIG_KEY_MAP } from "../api.js";
import { formatBytes } from "../format.js";

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

  // Treat null/undefined and "auto" as equivalent for serverId/interface
  if ((prevVal == null || prevVal === "auto") && (nextVal == null || nextVal === "auto" || nextVal === "")) {
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
 * Renders the Settings view with Schedule, Expected Plan, Language, Password Protection, Prometheus, and Data Management cards.
 *
 * @param {object} state - Application state
 * @param {object} [opts]
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t] - Translator function
 * @param {Array<{ code: string, name: string }>} [opts.locales] - Available languages list
 * @returns {string} HTML string
 */
export function renderSettings(state, { t = (k) => k, locales = [] } = {}) {
  const config = state?.config || {};
  const locale = state?.locale || "pt-BR";
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
  <div class="form-group">
    <label class="form-label" for="setting-backup-keep">${escapeHtml(t("settings.backupKeep") || "Backups diários mantidos")}</label>
    <input id="setting-backup-keep" type="number" name="backupKeep" min="0" max="30" value="${escapeHtml(config.backupKeep ?? 7)}" class="form-control" style="max-width: 200px;">
    <span class="form-help">${escapeHtml(t("settings.backupKeepHelp") || "0 desliga o backup diário do banco")}</span>
  </div>
</div>`;

  // 2. Plano / Valores esperados card (expectedMode, expectedDownload, expectedUpload, expectedPing)
  const mode = config.expectedMode || "auto";
  const isAuto = mode === "auto";
  const dlVal = config.expectedDownload != null ? config.expectedDownload : "";
  const ulVal = config.expectedUpload != null ? config.expectedUpload : "";
  const pctVal = config.degradedThresholdPct != null ? config.degradedThresholdPct : "";
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
    <div class="form-group">
      <label class="form-label" for="setting-degraded-pct">${escapeHtml(t("settings.degradedThresholdPct") || "Limite de lentidão (% do esperado)")}</label>
      <input id="setting-degraded-pct" type="number" step="1" min="1" max="100" name="degradedThresholdPct" value="${escapeHtml(pctVal)}" class="form-control">
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

  // Submit button for main config form
  const submitBtnHtml = `<div style="display: flex; justify-content: flex-end; margin-top: 16px; margin-bottom: 24px;">
  <button type="submit" class="btn btn-primary">${escapeHtml(t("common.save") || "Salvar alterações")}</button>
</div>`;

  // 4. Password Protection & Prometheus Metrics Card
  const currentPasswordMode = config.passwordMode || state?.auth?.mode || "none";
  const securityCardHtml = `<div class="card">
  <div class="card-header">
    <h2 class="card-title">${escapeHtml(t("settings.securityTitle") || "Segurança e Acesso")}</h2>
  </div>
  <form data-form="auth-password">
    <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 14px;">
      <div class="form-group">
        <label class="form-label" for="setting-password-mode">${escapeHtml(t("settings.passwordMode") || "Modo de proteção")}</label>
        <select id="setting-password-mode" name="mode" class="form-control">
          <option value="none"${currentPasswordMode === "none" ? " selected" : ""}>${escapeHtml(t("settings.passwordModeNone") || "Desativado (sem senha)")}</option>
          <option value="public"${currentPasswordMode === "public" ? " selected" : ""}>${escapeHtml(t("settings.passwordModePublic") || "Público (leitura livre, ajustes protegidos)")}</option>
          <option value="protected"${currentPasswordMode === "protected" ? " selected" : ""}>${escapeHtml(t("settings.passwordModeProtected") || "Protegido (exige senha para tudo)")}</option>
        </select>
      </div>
      <div class="form-group">
        <label class="form-label" for="setting-new-password">${escapeHtml(t("settings.newPassword") || "Nova senha")}</label>
        <input id="setting-new-password" type="password" name="password" class="form-control" placeholder="••••••••">
        <span class="form-help">${escapeHtml(t("settings.newPasswordHelp") || "Deixe em branco para manter a senha atual ao trocar de modo")}</span>
      </div>
    </div>
    <div style="display: flex; justify-content: flex-end; margin-top: 12px;">
      <button type="submit" class="btn btn-secondary">${escapeHtml(t("settings.savePassword") || "Atualizar segurança")}</button>
    </div>
  </form>
  <div style="border-top: 1px solid var(--border); margin-top: 16px; padding-top: 14px;">
    <div class="form-label">${escapeHtml(t("settings.prometheusTitle") || "Métricas Prometheus")}</div>
    <p class="form-help" style="margin-top: 4px;">
      ${escapeHtml(t("settings.prometheusHelp") || "Endpoint compatível com Prometheus (suporta HTTP Basic Auth quando protegido por senha):")}
      <code style="font-family: var(--font-mono); color: var(--accent-dl); margin-left: 6px;">/api/prometheus/metrics</code>
    </p>
  </div>
</div>`;

  // 5. Data Management Card (Export JSON/CSV, Import Netrack/MySpeed/CSV, Stats & Reset)
  const rowCount = state?.dataStats?.rowCount != null ? String(state.dataStats.rowCount) : "—";
  const dbSize = state?.dataStats?.dbSizeBytes != null ? formatBytes(state.dataStats.dbSizeBytes, locale) : "—";
  const activeNodeId = state?.nodes?.activeNodeId;
  const exportBase =
    activeNodeId != null && activeNodeId !== "" && activeNodeId !== "local"
      ? `/api/nodes/${encodeURIComponent(activeNodeId)}/proxy/data/export`
      : "/api/data/export";

  const dataCardHtml = `<div class="card">
  <div class="card-header">
    <h2 class="card-title">${escapeHtml(t("settings.dataTitle") || "Gerenciamento de Dados")}</h2>
  </div>

  <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; margin-bottom: 16px;">
    <div class="stat-kpi-card">
      <div class="stat-kpi-label">${escapeHtml(t("settings.dataTotalRecords") || "Total de registros")}</div>
      <div class="stat-kpi-value">${escapeHtml(rowCount)}</div>
    </div>
    <div class="stat-kpi-card">
      <div class="stat-kpi-label">${escapeHtml(t("settings.dataDbSize") || "Tamanho do banco")}</div>
      <div class="stat-kpi-value">${escapeHtml(dbSize)}</div>
    </div>
  </div>

  <div class="form-group">
    <div class="form-label">${escapeHtml(t("settings.exportTitle") || "Exportar histórico")}</div>
    <div style="display: flex; gap: 8px; flex-wrap: wrap; margin-top: 4px;">
      <a href="${escapeHtml(exportBase)}?format=json" download class="btn btn-secondary" data-action="export-json">${escapeHtml(t("settings.exportJson") || "Exportar JSON")}</a>
      <a href="${escapeHtml(exportBase)}?format=csv" download class="btn btn-secondary" data-action="export-csv">${escapeHtml(t("settings.exportCsv") || "Exportar CSV")}</a>
    </div>
  </div>

  <div style="border-top: 1px solid var(--border); margin-top: 16px; padding-top: 14px;">
    <form data-form="data-import">
      <div class="form-group">
        <label class="form-label" for="data-import-file">${escapeHtml(t("settings.importTitle") || "Importar histórico (Netrack JSON/CSV ou MySpeed JSON)")}</label>
        <div style="display: flex; gap: 8px; align-items: center; flex-wrap: wrap;">
          <input id="data-import-file" type="file" name="importFile" accept=".json,.csv,application/json,text/csv" class="form-control" style="max-width: 340px;">
          <button type="submit" class="btn btn-secondary" data-action="import-data">${escapeHtml(t("settings.importBtn") || "Importar arquivo")}</button>
        </div>
      </div>
    </form>
  </div>

  <div style="border-top: 1px solid var(--border); margin-top: 16px; padding-top: 14px; display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 12px;">
    <div>
      <div class="form-label" style="color: var(--accent-ping);">${escapeHtml(t("settings.resetTitle") || "Redefinir banco de dados")}</div>
      <div class="form-help">${escapeHtml(t("settings.resetHelp") || "Apaga todos os testes registrados e restaura as configurações padrão.")}</div>
    </div>
    <button type="button" class="btn btn-danger" data-action="reset-data">${escapeHtml(t("settings.resetBtn") || "Redefinir tudo")}</button>
  </div>
</div>`;

  return `${headerHtml}
<form data-form="settings">
  ${scheduleCardHtml}
  ${expectedCardHtml}
  ${languageCardHtml}
  ${submitBtnHtml}
</form>
${securityCardHtml}
${dataCardHtml}`;
}

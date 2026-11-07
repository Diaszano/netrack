// web/app.js
// Netrack web application wiring: router, event delegation, live SSE and polling

import { createApiClient } from "./api.js";
import { createInitialState, applyRunEvent, parseRoute } from "./state.js";
import { resolveLocale, createTranslator, escapeHtml } from "./i18n.js";
import { formatBytes, formatNumber, formatDateTime, dateRangeForPreset } from "./format.js";
import { renderOverview } from "./views/overview.js";
import { renderStatistics } from "./views/statistics.js";
import { renderHistory } from "./views/history.js";
import { renderServers } from "./views/servers.js";
import { renderSettings, saveConfigDiff } from "./views/settings.js";
import { renderNodes, renderIntegrations } from "./views/seams.js";

const apiClient = createApiClient();
let state = createInitialState();
let localesList = [];
let t = (k) => k;
let chartTooltipEl = null;

/**
 * Initializes i18n catalogs and translator.
 */
export async function setupI18n(customClient = apiClient, targetState = state) {
  try {
    localesList = await customClient.getLocales();
  } catch {
    localesList = [
      { code: "pt-BR", name: "Português (Brasil)" },
      { code: "en", name: "English" },
    ];
  }

  const availableCodes = localesList.map((l) => l.code);
  const savedPref = typeof localStorage !== "undefined"
    ? (localStorage.getItem("netrack.locale") || localStorage.getItem("netrack_locale"))
    : null;
  targetState.localePreference = savedPref || "auto";

  const navLangs = typeof navigator !== "undefined" && navigator.languages
    ? navigator.languages
    : (typeof navigator !== "undefined" && navigator.language ? [navigator.language] : []);

  const resolved = resolveLocale({
    saved: savedPref,
    navigatorLanguages: navLangs,
    available: availableCodes,
    defaultLocale: "pt-BR",
  });

  targetState.locale = resolved;

  let dict = {};
  let fallbackDict = {};
  try {
    dict = await customClient.getLocaleDict(resolved);
  } catch {}

  if (resolved !== "pt-BR") {
    try {
      fallbackDict = await customClient.getLocaleDict("pt-BR");
    } catch {}
  }

  t = createTranslator(dict, fallbackDict);

  if (typeof document !== "undefined") {
    document.documentElement.lang = resolved;
  }
}

/**
 * Loads data required for the specified route.
 */
export async function loadViewData(route, client = apiClient, appState = state) {
  try {
    switch (route) {
      case "overview": {
        const [recent, stats, status] = await Promise.all([
          client.listSpeedtests({ limit: 10 }).catch(() => []),
          (async () => {
            const range = dateRangeForPreset("7d", Date.now(), appState?.system?.tz);
            return client.getStatistics(range.from, range.to).catch(() => null);
          })(),
          typeof client.getStatus === "function" ? client.getStatus().catch(() => null) : Promise.resolve(null),
        ]);
        appState.recentTests = recent || [];
        appState.overviewStats = stats;
        if (status) {
          appState.status = status;
        }
        break;
      }
      case "statistics": {
        const preset = appState.statsRange.preset || "7d";
        if (!appState.statsRange.from || !appState.statsRange.to) {
          const range = dateRangeForPreset(preset, Date.now(), appState?.system?.tz);
          appState.statsRange.from = range.from;
          appState.statsRange.to = range.to;
        }
        appState.statsData = await client.getStatistics(
          appState.statsRange.from,
          appState.statsRange.to
        ).catch(() => null);
        break;
      }
      case "history": {
        appState.history.loading = true;
        const rows = await client.listSpeedtests({ limit: 50 }).catch(() => []);
        appState.history.rows = rows || [];
        appState.history.hasMore = (rows || []).length >= 50;
        appState.history.loading = false;
        break;
      }
      case "servers": {
        if (!appState.config) {
          appState.config = await client.getConfig().catch(() => null);
        }
        const [ifaces, libre] = await Promise.all([
          client.getInterfaces().catch(() => []),
          client.getLibreServers().then(
            (res) => ({ ok: true, data: res }),
            (err) => ({ ok: false, error: err.message || "Failed to load LibreSpeed servers" })
          ),
        ]);
        appState.servers.interfaces = ifaces || [];
        if (libre.ok) {
          appState.servers.libreServers = libre.data || [];
          appState.servers.libreError = null;
        } else {
          appState.servers.libreServers = [];
          appState.servers.libreError = libre.error;
        }
        break;
      }
      case "settings": {
        if (!appState.config) {
          appState.config = await client.getConfig().catch(() => null);
        }
        break;
      }
      case "nodes":
      case "integrations":
      default:
        break;
    }
  } catch (err) {
    console.error("Failed to load view data for", route, err);
  }
}

/**
 * Updates the sidebar footer server status card.
 */
function updateSidebarFooter() {
  const versionEl = document.getElementById("server-version");
  if (versionEl) {
    versionEl.textContent = `v${state.system.version || "—"}`;
  }

  const indicatorEl = document.getElementById("server-status-indicator");
  const labelEl = document.getElementById("server-status-label");
  if (indicatorEl && labelEl) {
    indicatorEl.className = "status-indicator online";
    labelEl.textContent = t("common.online") || "online";
  }

  const metricsEl = document.getElementById("server-stats-metrics");
  if (metricsEl) {
    const cpuStr = state.system.cpuPercent != null
      ? formatNumber(state.system.cpuPercent, state.locale, 1)
      : "—";
    const ramStr = formatBytes(state.system.rssBytes, state.locale);
    metricsEl.textContent = `CPU ${cpuStr}% · RAM ${ramStr}`;
  }
}

/**
 * Updates the topbar title and action area.
 */
function updateTopbar() {
  const pageTitleEl = document.getElementById("page-title");
  if (pageTitleEl) {
    pageTitleEl.textContent = t(`nav.${state.route}`) || state.route;
  }
}

/**
 * Updates the notification/error banner container.
 */
export function updateBanner() {
  const bannerContainer = document.getElementById("banner-container");
  if (!bannerContainer) return;

  if (!state.banner || !state.banner.text) {
    bannerContainer.innerHTML = "";
    return;
  }

  const kind = state.banner.kind || "info";
  bannerContainer.innerHTML = `<div class="banner banner-${escapeHtml(kind)}">
  <span>${escapeHtml(state.banner.text)}</span>
  <button type="button" class="banner-close" data-action="close-banner" aria-label="Close">&times;</button>
</div>`;
}

/**
 * Renders the active SPA view into #main-content and synchronizes UI chrome.
 */
export function render() {
  if (typeof document === "undefined") return;

  // 1. Sidebar Nav active state & i18n
  document.querySelectorAll(".sidebar-nav .nav-item").forEach((el) => {
    const route = el.getAttribute("data-route");
    el.classList.toggle("active", route === state.route);
  });

  document.querySelectorAll("[data-i18n]").forEach((el) => {
    const key = el.getAttribute("data-i18n");
    if (key) {
      el.textContent = t(key);
    }
  });

  // 2. Topbar, Footer & Banner
  updateTopbar();
  updateSidebarFooter();
  updateBanner();

  // 3. View content mount
  const mainEl = document.getElementById("main-content");
  if (!mainEl) return;

  let viewHtml = "";
  switch (state.route) {
    case "overview":
      viewHtml = renderOverview(state, { t });
      break;
    case "statistics":
      viewHtml = renderStatistics(state, { t });
      break;
    case "history":
      viewHtml = renderHistory(state, { t });
      break;
    case "servers":
      viewHtml = renderServers(state, { t });
      break;
    case "nodes":
      viewHtml = renderNodes(state, { t });
      break;
    case "integrations":
      viewHtml = renderIntegrations(state, { t });
      break;
    case "settings":
      viewHtml = renderSettings(state, { t, locales: localesList });
      break;
    default:
      viewHtml = renderOverview(state, { t });
      break;
  }

  mainEl.innerHTML = viewHtml;
}

/**
 * Navigates to a new route, updating URL and history state.
 */
async function navigateTo(route, urlPath) {
  const targetPath = urlPath || (route === "overview" ? "/" : `/${route}`);
  if (window.location.pathname !== targetPath) {
    history.pushState(null, "", targetPath);
  }
  state.route = route;

  // Close mobile drawer if open
  closeMobileMenu();

  await loadViewData(state.route);
  render();
}

/**
 * Closes the mobile navigation drawer.
 */
function closeMobileMenu() {
  const sidebar = document.getElementById("sidebar");
  const backdrop = document.getElementById("sidebar-backdrop");
  if (sidebar) sidebar.classList.remove("open");
  if (backdrop) backdrop.classList.remove("open");
}

/**
 * Opens the mobile navigation drawer.
 */
function openMobileMenu() {
  const sidebar = document.getElementById("sidebar");
  const backdrop = document.getElementById("sidebar-backdrop");
  if (sidebar) sidebar.classList.add("open");
  if (backdrop) backdrop.classList.add("open");
}

/**
 * Connects to the Server-Sent Events stream for live test execution events.
 */
function setupEventSource() {
  if (typeof EventSource === "undefined") return;

  try {
    const es = new EventSource("/api/speedtests/events");

    es.onmessage = async (e) => {
      try {
        const event = JSON.parse(e.data);
        const { nextState, shouldRefresh } = applyRunEvent(state, event);
        state = nextState;

        if (shouldRefresh) {
          const [status, cfg] = await Promise.all([
            apiClient.getStatus().catch(() => null),
            apiClient.getConfig().catch(() => null),
          ]);
          if (status) state.status = status;
          if (cfg) state.config = cfg;
          await loadViewData(state.route);
        }

        render();
      } catch (err) {
        console.error("Failed to parse SSE event", err);
      }
    };

    es.onerror = () => {
      // EventSource reconnects automatically on failure
    };
  } catch (err) {
    console.warn("SSE connection error", err);
  }
}

/**
 * Periodically polls system status every 15 seconds.
 */
function setupPolling() {
  setInterval(async () => {
    try {
      const sys = await apiClient.getSystemStatus();
      if (sys) {
        state.system = sys;
        updateSidebarFooter();
      }
    } catch {
      const indicatorEl = document.getElementById("server-status-indicator");
      const labelEl = document.getElementById("server-status-label");
      if (indicatorEl && labelEl) {
        indicatorEl.className = "status-indicator offline";
        labelEl.textContent = t("common.offline") || "offline";
      }
    }
  }, 15000);
}

/**
 * Creates and initializes the floating chart tooltip element.
 */
function ensureChartTooltip() {
  if (chartTooltipEl || typeof document === "undefined") return;
  chartTooltipEl = document.createElement("div");
  chartTooltipEl.id = "chart-tooltip";
  chartTooltipEl.style.cssText = `
    position: fixed;
    display: none;
    pointer-events: none;
    z-index: 1000;
    background-color: #171A31;
    border: 1px solid #262A47;
    border-radius: 4px;
    padding: 6px 10px;
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    color: #E8E9F5;
    box-shadow: 0 4px 12px rgba(0,0,0,0.4);
    white-space: nowrap;
  `;
  document.body.appendChild(chartTooltipEl);
}

/**
 * Extracts Servers form values from FormData, preserving LibreSpeed settings when provider is Cloudflare
 * (since disabled controls are omitted from FormData).
 */
export function extractServersFormValues(formData, currentConfig = {}) {
  return {
    provider: formData.get("provider") || "cloudflare",
    libreServerId: formData.get("libreServerId") ?? currentConfig?.libreServerId ?? "auto",
    libreUrl: formData.get("libreUrl") ?? currentConfig?.libreUrl ?? "",
    interface: formData.get("interface") || "auto",
  };
}

/**
 * Extracts Settings form values from FormData without silently coercing empty strings.
 */
export function extractSettingsFormValues(formData) {
  const rawDl = formData.get("expectedDownload");
  const rawUl = formData.get("expectedUpload");
  const rawPing = formData.get("expectedPing");
  const rawRetention = formData.get("retentionDays");

  return {
    cron: formData.get("cron") ?? "",
    scheduleOffset: formData.has("scheduleOffset"),
    paused: formData.has("paused"),
    retentionDays: rawRetention !== "" && rawRetention != null ? Number(rawRetention) : "",
    expectedMode: formData.get("expectedMode") || "auto",
    expectedDownload: rawDl !== "" && rawDl != null ? Number(rawDl) : null,
    expectedUpload: rawUl !== "" && rawUl != null ? Number(rawUl) : null,
    expectedPing: rawPing !== "" && rawPing != null ? Number(rawPing) : null,
  };
}

/**
 * Handles Servers form submission. On success, updates state and calls onSaveSuccess (e.g. render).
 * On failure, updates state banner and calls onSaveError (e.g. updateBanner) to preserve form inputs.
 */
export async function onServersSubmit(formData, appState, client, { t = (k) => k, onSaveSuccess, onSaveError } = {}) {
  const formValues = extractServersFormValues(formData, appState?.config);
  const result = await saveConfigDiff(appState?.config, formValues, client);
  if (result.ok) {
    if (appState) {
      appState.config = { ...appState.config, ...formValues };
      appState.banner = { kind: "info", text: t("settings.saveSuccess") || "Salvo com sucesso!" };
    }
    if (typeof onSaveSuccess === "function") {
      onSaveSuccess(result);
    }
  } else {
    if (appState) {
      appState.banner = { kind: "error", text: result.error || "Erro ao salvar" };
    }
    if (typeof onSaveError === "function") {
      onSaveError(result);
    }
  }
  return result;
}

/**
 * Handles Settings form submission. On success, updates state and calls onSaveSuccess (e.g. render).
 * On failure, updates state banner and calls onSaveError (e.g. updateBanner) to preserve form inputs.
 */
export async function onSettingsSubmit(formData, appState, client, { t = (k) => k, onSaveSuccess, onSaveError, onLocaleChange } = {}) {
  const formValues = extractSettingsFormValues(formData);
  const newLocale = formData.get("locale");
  if (newLocale && newLocale !== appState?.localePreference) {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem("netrack.locale", newLocale);
    }
    if (appState) {
      appState.localePreference = newLocale;
    }
    if (typeof onLocaleChange === "function") {
      await onLocaleChange(newLocale);
    }
  }

  const result = await saveConfigDiff(appState?.config, formValues, client);
  if (result.ok) {
    if (appState) {
      appState.config = { ...appState.config, ...formValues };
      appState.banner = { kind: "info", text: t("settings.saveSuccess") || "Ajustes salvos com sucesso!" };
      if (typeof client?.getStatus === "function") {
        try {
          const freshStatus = await client.getStatus();
          if (freshStatus) {
            appState.status = freshStatus;
          }
        } catch {}
      }
    }
    if (typeof onSaveSuccess === "function") {
      onSaveSuccess(result);
    }
  } else {
    if (appState) {
      appState.banner = { kind: "error", text: result.error || "Erro ao salvar" };
    }
    if (typeof onSaveError === "function") {
      onSaveError(result);
    }
  }
  return result;
}

/**
 * Sets up global event delegation for clicks, submits, changes and hover tooltips.
 */
function setupEventDelegation() {
  ensureChartTooltip();

  // 1. Click delegation
  document.addEventListener("click", async (e) => {
    // Mobile menu toggle
    if (e.target.closest("#menu-toggle")) {
      e.preventDefault();
      openMobileMenu();
      return;
    }

    // Mobile menu close
    if (e.target.closest("#sidebar-close") || e.target.closest("#sidebar-backdrop")) {
      e.preventDefault();
      closeMobileMenu();
      return;
    }

    // Internal navigation links
    const link = e.target.closest("a[data-route], a.brand-link, .sidebar-nav a");
    if (link && !e.ctrlKey && !e.metaKey && !e.shiftKey) {
      const href = link.getAttribute("href");
      if (href && href.startsWith("/") && !href.startsWith("//")) {
        e.preventDefault();
        const route = link.getAttribute("data-route") || parseRoute(href);
        await navigateTo(route, href);
        return;
      }
    }

    // Action: close-banner
    if (e.target.closest('[data-action="close-banner"]')) {
      e.preventDefault();
      state.banner = null;
      updateBanner();
      return;
    }

    // Action: run-test
    if (e.target.closest('[data-action="run-test"]')) {
      e.preventDefault();
      state.status.running = true;
      render();
      try {
        await apiClient.runSpeedtest();
      } catch (err) {
        state.status.running = false;
        state.banner = { kind: "error", text: err.message || "Speed test failed to start" };
        render();
      }
      return;
    }

    // Action: delete-test
    const delBtn = e.target.closest('[data-action="delete-test"]');
    if (delBtn) {
      e.preventDefault();
      const id = delBtn.getAttribute("data-id");
      const confirmMsg = t("history.deleteConfirm") || "Tem certeza que deseja excluir este teste?";
      if (confirm(confirmMsg)) {
        try {
          await apiClient.deleteSpeedtest(id);
          state.history.rows = (state.history.rows || []).filter((r) => String(r.id) !== String(id));
          state.recentTests = (state.recentTests || []).filter((r) => String(r.id) !== String(id));
          render();
        } catch (err) {
          state.banner = { kind: "error", text: err.message || "Failed to delete test" };
          render();
        }
      }
      return;
    }

    // Action: load-more-history
    if (e.target.closest('[data-action="load-more-history"]')) {
      e.preventDefault();
      const rows = state.history.rows || [];
      const oldest = rows[rows.length - 1];
      const beforeId = oldest ? (oldest.id ?? oldest.t) : undefined;

      state.history.loading = true;
      render();

      try {
        const more = await apiClient.listSpeedtests({ before: beforeId, limit: 50 });
        const moreList = Array.isArray(more) ? more : [];
        state.history.rows = [...rows, ...moreList];
        state.history.hasMore = moreList.length >= 50;
      } catch (err) {
        state.banner = { kind: "error", text: err.message || "Failed to load more tests" };
      } finally {
        state.history.loading = false;
        render();
      }
      return;
    }

    // Action: stats-preset
    const presetBtn = e.target.closest('[data-action="stats-preset"], [data-preset]');
    if (presetBtn && presetBtn.getAttribute("data-preset")) {
      e.preventDefault();
      const preset = presetBtn.getAttribute("data-preset");
      state.statsRange.preset = preset;
      const range = dateRangeForPreset(preset, Date.now(), state.system.tz);
      state.statsRange.from = range.from;
      state.statsRange.to = range.to;
      render();

      try {
        state.statsData = await apiClient.getStatistics(range.from, range.to);
      } catch (err) {
        state.banner = { kind: "error", text: err.message || "Failed to load statistics" };
      }
      render();
      return;
    }
  });

  // 2. Form submission delegation
  document.addEventListener("submit", async (e) => {
    const form = e.target;
    if (!form || !form.dataset) return;

    // Servers form submit
    if (form.dataset.form === "servers") {
      e.preventDefault();
      const fd = new FormData(form);
      await onServersSubmit(fd, state, apiClient, {
        t,
        onSaveSuccess: () => render(),
        onSaveError: () => updateBanner(),
      });
      return;
    }

    // Settings form submit
    if (form.dataset.form === "settings") {
      e.preventDefault();
      const fd = new FormData(form);
      await onSettingsSubmit(fd, state, apiClient, {
        t,
        onSaveSuccess: () => render(),
        onSaveError: () => updateBanner(),
        onLocaleChange: async () => {
          await setupI18n();
        },
      });
      return;
    }
  });

  // 3. Change event delegation
  document.addEventListener("change", async (e) => {
    const target = e.target;
    if (!target) return;

    // Expected mode toggle in Settings
    if (target.name === "expectedMode") {
      const isAuto = target.value === "auto";
      const form = target.closest("form");
      if (form) {
        ["expectedDownload", "expectedUpload", "expectedPing"].forEach((name) => {
          const input = form.querySelector(`[name="${name}"]`);
          if (input) {
            input.readOnly = isAuto;
          }
        });
      }
      return;
    }

    // Provider radio switch in Servers
    if (target.name === "provider") {
      const isLibre = target.value === "librespeed";
      const card = document.getElementById("librespeed-card");
      if (card) {
        card.style.opacity = isLibre ? "1" : "0.6";
        card.querySelectorAll("input, select").forEach((el) => {
          el.disabled = !isLibre;
        });
      }
      return;
    }

    // Date range inputs in Statistics
    if (target.name === "from" || target.name === "to") {
      const fromEl = document.querySelector('input[name="from"]');
      const toEl = document.querySelector('input[name="to"]');
      if (fromEl && toEl && fromEl.value && toEl.value) {
        state.statsRange.from = fromEl.value;
        state.statsRange.to = toEl.value;
        state.statsRange.preset = "";
        try {
          state.statsData = await apiClient.getStatistics(fromEl.value, toEl.value);
        } catch (err) {
          state.banner = { kind: "error", text: err.message || "Failed to load statistics" };
        }
        render();
      }
      return;
    }
  });

  // 4. Hover tooltips on chart points
  document.addEventListener("pointerover", (e) => {
    const point = e.target.closest(".chart-point");
    if (!point || !chartTooltipEl) return;

    const tMs = point.getAttribute("data-t");
    const dl = point.getAttribute("data-dl");
    const ul = point.getAttribute("data-ul");
    const ping = point.getAttribute("data-ping");

    const timeStr = tMs ? formatDateTime(Number(tMs), state.locale, state.system.tz) : "";
    let content = `<div>${escapeHtml(timeStr)}</div>`;
    if (dl) content += `<div style="color: #8B7CFF;">↓ ${escapeHtml(dl)} Mbps</div>`;
    if (ul) content += `<div style="color: #3FD6C2;">↑ ${escapeHtml(ul)} Mbps</div>`;
    if (ping) content += `<div style="color: #FF8FB1;">• ${escapeHtml(ping)} ms</div>`;

    chartTooltipEl.innerHTML = content;
    chartTooltipEl.style.display = "block";

    const rect = point.getBoundingClientRect();
    const tipWidth = chartTooltipEl.offsetWidth || 140;
    let left = rect.left + window.scrollX - tipWidth / 2 + 4;
    let top = rect.top + window.scrollY - 75;

    if (left < 10) left = 10;
    if (left + tipWidth > window.innerWidth - 10) left = window.innerWidth - tipWidth - 10;
    if (top < 10) top = rect.bottom + 10;

    chartTooltipEl.style.left = `${left}px`;
    chartTooltipEl.style.top = `${top}px`;
  });

  document.addEventListener("pointerout", (e) => {
    const point = e.target.closest(".chart-point");
    if (point && chartTooltipEl) {
      chartTooltipEl.style.display = "none";
    }
  });

  // 5. Browser history popstate
  window.addEventListener("popstate", async () => {
    state.route = parseRoute(window.location.pathname);
    await loadViewData(state.route);
    render();
  });
}

/**
 * Main application bootstrap entry point.
 */
async function main() {
  if (typeof window === "undefined" || typeof document === "undefined") return;

  // Initialize route from current browser URL
  state.route = parseRoute(window.location.pathname);

  // Setup i18n
  await setupI18n();

  // Load initial global data
  const [sys, status, cfg] = await Promise.all([
    apiClient.getSystemStatus().catch(() => null),
    apiClient.getStatus().catch(() => null),
    apiClient.getConfig().catch(() => null),
  ]);

  if (sys) state.system = sys;
  if (status) state.status = status;
  if (cfg) state.config = cfg;

  // Load route-specific initial data
  await loadViewData(state.route);

  // Setup UI event listeners, SSE stream and periodic polling
  setupEventDelegation();
  setupEventSource();
  setupPolling();

  // Initial render
  render();
}

// Auto-run bootstrap if running in a browser environment
if (typeof window !== "undefined" && typeof document !== "undefined") {
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", main);
  } else {
    main();
  }
}

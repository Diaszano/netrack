// web/format.js
// Number, date, time, byte, and metric formatters with server timezone awareness

/**
 * Formats a numeric value using Intl.NumberFormat in the given locale.
 * Returns em-dash "—" if the value is null, undefined, or not finite.
 *
 * @param {number|null|undefined} value
 * @param {string} locale
 * @param {number} [decimals=0]
 * @returns {string}
 */
export function formatNumber(value, locale, decimals = 0) {
  if (value == null || !Number.isFinite(value)) {
    return "—";
  }
  return new Intl.NumberFormat(locale, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  }).format(value);
}

/**
 * Formats a metric value (download, upload, ping, jitter) with contextual decimals.
 * - 0 decimals for integer-like values or >= 100 Mbps/ms on gauges.
 * - 1 decimal for jitter (e.g. "1,8" in pt-BR).
 * - Explicit `decimals` if provided.
 *
 * @param {number|null|undefined} value
 * @param {string} locale
 * @param {"download"|"upload"|"ping"|"jitter"} metric
 * @param {number} [decimals]
 * @returns {string}
 */
export function formatMetric(value, locale, metric, decimals) {
  if (value == null || !Number.isFinite(value)) {
    return "—";
  }

  let d = decimals;
  if (d === undefined) {
    if (metric === "jitter") {
      d = 1;
    } else if (Number.isInteger(value) || Math.abs(value) >= 100) {
      d = 0;
    } else {
      d = 1;
    }
  }

  return formatNumber(value, locale, d);
}

/**
 * Parses timestamp input into a valid Date object, or null.
 *
 * @param {number|string|Date|null|undefined} ms
 * @returns {Date|null}
 */
function toDate(ms) {
  if (ms == null) return null;
  const d = ms instanceof Date ? ms : new Date(typeof ms === "string" && !isNaN(Number(ms)) ? Number(ms) : ms);
  return Number.isFinite(d.getTime()) ? d : null;
}

/**
 * Formats a timestamp into HH:MM in the specified timezone.
 *
 * @param {number|string|Date} ms
 * @param {string} locale
 * @param {string} tz
 * @returns {string}
 */
export function formatTime(ms, locale, tz) {
  const date = toDate(ms);
  if (!date) return "—";
  return new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
    timeZone: tz,
  }).format(date);
}

/**
 * Formats a timestamp into short date + HH:MM in the specified timezone.
 *
 * @param {number|string|Date} ms
 * @param {string} locale
 * @param {string} tz
 * @returns {string}
 */
export function formatDateTime(ms, locale, tz) {
  const date = toDate(ms);
  if (!date) return "—";
  return new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
    timeZone: tz,
  }).format(date);
}

/**
 * Formats a timestamp into a YYYY-MM-DD calendar date in the specified timezone.
 * Useful for HTML `<input type="date">` values.
 *
 * @param {number|string|Date} ms
 * @param {string} tz
 * @returns {string}
 */
export function formatDateInput(ms, tz) {
  const date = toDate(ms);
  if (!date) return "";
  return new Intl.DateTimeFormat("en-CA", {
    timeZone: tz,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(date);
}

const PRESET_DAYS = {
  "24h": 1,
  "7d": 6,
  "30d": 29,
  "365d": 364,
};

/**
 * Computes `{ from, to }` YYYY-MM-DD range for a statistics preset in `tz`.
 * `to` is today in `tz`; `from` is `to` minus preset days (inclusive calendar days).
 *
 * @param {"24h"|"7d"|"30d"|"365d"} preset
 * @param {number} nowMs
 * @param {string} tz
 * @returns {{ from: string, to: string }}
 */
export function dateRangeForPreset(preset, nowMs, tz) {
  const to = formatDateInput(nowMs, tz);
  const days = PRESET_DAYS[preset] ?? 6;

  const [year, month, day] = to.split("-").map(Number);
  const fromDate = new Date(Date.UTC(year, month - 1, day - days));
  const from = fromDate.toISOString().slice(0, 10);

  return { from, to };
}

const BYTES_IN_GB = 1024 * 1024 * 1024;
const BYTES_IN_MB = 1024 * 1024;

/**
 * Formats a byte count into MB or GB. Returns "—" on null, undefined, or negative values.
 *
 * @param {number|null|undefined} bytes
 * @param {string} locale
 * @returns {string}
 */
export function formatBytes(bytes, locale) {
  if (bytes == null || !Number.isFinite(bytes) || bytes < 0) {
    return "—";
  }
  if (bytes >= BYTES_IN_GB) {
    const gb = bytes / BYTES_IN_GB;
    return `${formatNumber(gb, locale, 1)} GB`;
  }
  const mb = Math.round(bytes / BYTES_IN_MB);
  return `${formatNumber(mb, locale, 0)} MB`;
}

/**
 * Computes plan progress and visual arc fill ratio.
 * For download/upload: ratio = Math.min(1, Math.max(0, actual / expected)), percent = Math.round(ratio * 100).
 * For ping: lower is better, ratio = Math.min(1, Math.max(0, expected / actual)).
 *
 * @param {number|null|undefined} actual
 * @param {number|null|undefined} expected
 * @param {"download"|"upload"|"ping"} metric
 * @returns {{ percent: number|null, ratio: number }}
 */
export function computePlanProgress(actual, expected, metric) {
  if (
    !Number.isFinite(actual) ||
    !Number.isFinite(expected) ||
    expected <= 0
  ) {
    return { percent: null, ratio: 0 };
  }

  if (metric === "download" || metric === "upload") {
    if (actual < 0) {
      return { percent: null, ratio: 0 };
    }
    const ratio = Math.min(1, Math.max(0, actual / expected));
    const percent = Math.round((actual / expected) * 100);
    return { percent, ratio };
  }

  if (metric === "ping") {
    if (actual <= 0) {
      return { percent: null, ratio: 0 };
    }
    const ratio = Math.min(1, Math.max(0, expected / actual));
    const percent = Math.round((expected / actual) * 100);
    return { percent, ratio };
  }

  return { percent: null, ratio: 0 };
}

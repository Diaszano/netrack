// web/i18n.js
// Client-side internationalization engine and string escaping

/**
 * Escapes HTML special characters to prevent XSS. Coerces non-strings safely.
 * Returns an empty string for null/undefined.
 *
 * @param {unknown} str
 * @returns {string}
 */
export function escapeHtml(str) {
  if (str == null) {
    return "";
  }
  return String(str)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

/**
 * Resolves active locale from saved preference, browser languages, and available catalogs.
 *
 * Priority:
 * 1. `saved` (if not "auto" and present in `available`, case-insensitive).
 * 2. `navigatorLanguages` in order: exact match first, then primary subtag match
 *    (e.g. "pt-PT" -> "pt-BR", "en-US" -> "en").
 * 3. `defaultLocale` ("pt-BR").
 *
 * @param {object} [opts]
 * @param {string|null} [opts.saved]
 * @param {readonly string[]} [opts.navigatorLanguages]
 * @param {readonly string[]} [opts.available]
 * @param {string} [opts.defaultLocale]
 * @returns {string}
 */
export function resolveLocale({
  saved,
  navigatorLanguages = [],
  available = ["pt-BR", "en"],
  defaultLocale = "pt-BR",
} = {}) {
  if (saved && saved !== "auto") {
    const savedLower = String(saved).toLowerCase();
    const savedMatch = available.find((a) => a.toLowerCase() === savedLower);
    if (savedMatch) {
      return savedMatch;
    }
  }

  for (const lang of navigatorLanguages) {
    if (!lang || typeof lang !== "string") continue;
    const langLower = lang.toLowerCase();

    // 1. Exact case-insensitive match
    const exact = available.find((a) => a.toLowerCase() === langLower);
    if (exact) {
      return exact;
    }

    // 2. Primary subtag match (e.g. "pt" or "en")
    const primary = langLower.split(/[-_]/)[0];
    const subtagMatch = available.find((a) => {
      const aLower = a.toLowerCase();
      return aLower === primary || aLower.split(/[-_]/)[0] === primary;
    });
    if (subtagMatch) {
      return subtagMatch;
    }
  }

  return defaultLocale;
}

/**
 * Creates a translator function `t(key, vars)` using the provided catalog and fallback catalog.
 * Interpolates `{name}` tokens with corresponding values in `vars`.
 * Returns non-string catalog entries (e.g. arrays) as-is without interpolation.
 *
 * @param {Record<string, unknown>} [dict]
 * @param {Record<string, unknown>} [fallbackDict]
 * @returns {(key: string, vars?: Record<string, string | number>) => any}
 */
export function createTranslator(dict = {}, fallbackDict = {}) {
  return function t(key, vars) {
    const template = dict?.[key] ?? fallbackDict?.[key] ?? key;
    if (typeof template !== "string") {
      return template;
    }
    if (!vars || typeof vars !== "object") {
      return template;
    }
    return template.replace(/\{([a-zA-Z0-9_]+)\}/g, (match, name) => {
      return vars[name] !== undefined ? String(vars[name]) : match;
    });
  };
}

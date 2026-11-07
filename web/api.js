/**
 * netrack API client
 */

export class ApiClientError extends Error {
  constructor(status, message) {
    super(message);
    this.name = "ApiClientError";
    this.status = status;
  }
}

export const CONFIG_KEY_MAP = {
  provider: "provider",
  cron: "cron",
  scheduleOffset: "schedule_offset",
  retentionDays: "retention_days",
  expectedMode: "expected_mode",
  expectedPing: "expected_ping",
  expectedDownload: "expected_download",
  expectedUpload: "expected_upload",
  libreServerId: "libre_server_id",
  libreUrl: "libre_url",
  interface: "interface",
  paused: "paused",
};

/**
 * Serializes a configuration value into the string format required by PATCH /api/config/{key}.
 * Converts null/empty string on libreServerId/interface to "auto",
 * converts null/empty string on libreUrl to "none",
 * converts booleans to "true"/"false",
 * and numbers/strings to String(val).
 */
export function serializeConfigValue(key, val) {
  if (
    key === "libreServerId" ||
    key === "libre_server_id" ||
    key === "interface"
  ) {
    if (val === null || val === undefined || val === "") {
      return "auto";
    }
  }

  if (key === "libreUrl" || key === "libre_url") {
    if (val === null || val === undefined || val === "") {
      return "none";
    }
  }

  if (typeof val === "boolean") {
    return val ? "true" : "false";
  }

  if (val === null || val === undefined) {
    return "";
  }

  return String(val);
}

/**
 * Creates an API client instance using the provided fetch implementation.
 */
export function createApiClient(fetchImpl = globalThis.fetch) {
  async function request(url, options = {}) {
    const res = await fetchImpl(url, options);

    if (!res.ok) {
      let message = res.statusText || `HTTP ${res.status}`;
      try {
        const data = await res.json();
        if (data && typeof data.error === "string") {
          message = data.error;
        }
      } catch {
        // Fall back to statusText if body is not JSON or already read
      }
      throw new ApiClientError(res.status, message);
    }

    if (res.status === 204) {
      return null;
    }

    const ct =
      res.headers && typeof res.headers.get === "function"
        ? res.headers.get("content-type")
        : "";

    if (ct && ct.includes("application/json")) {
      return res.json();
    }

    if (typeof res.text === "function") {
      const text = await res.text();
      if (!text) return null;
      try {
        return JSON.parse(text);
      } catch {
        return text;
      }
    }

    if (typeof res.json === "function") {
      return res.json();
    }

    return null;
  }

  return {
    async getStatus() {
      return request("/api/speedtests/status");
    },

    async runSpeedtest() {
      return request("/api/speedtests/run", { method: "POST" });
    },

    async pause() {
      return request("/api/speedtests/pause", { method: "POST" });
    },

    async resume() {
      return request("/api/speedtests/continue", { method: "POST" });
    },

    async listSpeedtests({ before, limit } = {}) {
      const params = new URLSearchParams();
      if (before != null) params.set("before", String(before));
      if (limit != null) params.set("limit", String(limit));
      const query = params.toString();
      return request(query ? `/api/speedtests?${query}` : "/api/speedtests");
    },

    async deleteSpeedtest(id) {
      return request(`/api/speedtests/${encodeURIComponent(id)}`, {
        method: "DELETE",
      });
    },

    async getStatistics(from, to) {
      const params = new URLSearchParams();
      if (from != null) params.set("from", String(from));
      if (to != null) params.set("to", String(to));
      return request(`/api/speedtests/statistics?${params.toString()}`);
    },

    async getConfig() {
      return request("/api/config");
    },

    async patchConfig(fieldOrKey, rawValue) {
      const key = CONFIG_KEY_MAP[fieldOrKey] ?? fieldOrKey;
      const value = serializeConfigValue(fieldOrKey, rawValue);
      return request(`/api/config/${encodeURIComponent(key)}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ value }),
      });
    },

    async getSystemStatus() {
      return request("/api/system/status");
    },

    async getInterfaces() {
      return request("/api/system/interfaces");
    },

    async getLibreServers() {
      return request("/api/system/servers/librespeed");
    },

    async getLocales() {
      return request("/locales/index.json");
    },

    async getLocaleDict(code) {
      return request(`/locales/${encodeURIComponent(code)}.json`);
    },
  };
}

/**
 * netrack application state & SSE event reducer
 */

export const VALID_ROUTES = new Set([
  "overview",
  "statistics",
  "history",
  "servers",
  "nodes",
  "integrations",
  "settings",
]);

/**
 * Creates the default initial state for the netrack SPA.
 */
export function createInitialState() {
  return {
    route: "overview",
    locale: "pt-BR",
    status: {
      running: false,
      paused: false,
      nextRun: null,
    },
    system: {
      version: "—",
      uptimeSecs: 0,
      cpuPercent: null,
      rssBytes: null,
      tz: "UTC",
    },
    config: null,
    recentTests: [],
    overviewStats: null,
    statsRange: {
      preset: "7d",
      from: "",
      to: "",
    },
    statsData: null,
    history: {
      rows: [],
      hasMore: false,
      loading: false,
    },
    servers: {
      interfaces: [],
      libreServers: [],
      libreError: null,
    },
    banner: null,
  };
}

/**
 * Pure state transition reducer for SSE RunEvents.
 * Returns { nextState, shouldRefresh }.
 */
export function applyRunEvent(state, event) {
  if (!event || typeof event !== "object") {
    return { nextState: state, shouldRefresh: false };
  }

  switch (event.type) {
    case "started":
      return {
        nextState: {
          ...state,
          status: {
            ...state.status,
            running: true,
          },
        },
        shouldRefresh: false,
      };

    case "finished":
      return {
        nextState: {
          ...state,
          status: {
            ...state.status,
            running: false,
          },
        },
        shouldRefresh: true,
      };

    case "failed":
      return {
        nextState: {
          ...state,
          status: {
            ...state.status,
            running: false,
          },
          banner: {
            kind: "error",
            text: event.error ?? "",
          },
        },
        shouldRefresh: true,
      };

    case "expectedUpdated": {
      if (!state.config) {
        return { nextState: state, shouldRefresh: false };
      }
      return {
        nextState: {
          ...state,
          config: {
            ...state.config,
            expectedPing: event.ping,
            expectedDownload: event.download,
            expectedUpload: event.upload,
          },
        },
        shouldRefresh: false,
      };
    }

    default:
      return { nextState: state, shouldRefresh: false };
  }
}

/**
 * Parses a browser pathname into a recognized SPA route name.
 * Defaults to "overview" for root or unrecognized routes.
 */
export function parseRoute(pathname) {
  if (!pathname || typeof pathname !== "string") {
    return "overview";
  }

  const withoutQuery = pathname.split("?")[0];
  const segments = withoutQuery.replace(/^\/+|\/+$/g, "").split("/");
  const first = segments[0] || "";

  if (VALID_ROUTES.has(first)) {
    return first;
  }

  return "overview";
}

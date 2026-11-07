// web/views/auth.js
// Authentication modal (protected/public login) and topbar auth badge

import { escapeHtml } from "../i18n.js";

/**
 * Renders the authentication login modal overlay when login is required or requested.
 *
 * @param {object} state - Application state
 * @param {object} [opts]
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t] - Translator function
 * @returns {string} HTML string
 */
export function renderAuthModal(state, { t = (k) => k } = {}) {
  const auth = state?.auth || { mode: "none", authenticated: false, modalOpen: false, error: null };
  const isProtectedLock = auth.mode === "protected" && !auth.authenticated;
  const isOpen = isProtectedLock || Boolean(auth.modalOpen && !auth.authenticated);

  if (!isOpen) {
    return "";
  }

  const title = t("auth.loginTitle") || "Autenticação necessária";
  const desc = isProtectedLock
    ? (t("auth.protectedDescription") || "Esta instância está protegida por senha. Digite sua senha para continuar.")
    : (t("auth.publicDescription") || "Digite sua senha de administrador para modificar configurações.");
  const passwordLabel = t("auth.password") || "Senha";
  const loginBtn = t("auth.login") || "Entrar";
  const cancelBtn = t("common.cancel") || "Cancelar";

  const errorBanner = auth.error
    ? `<div class="banner banner-error" style="margin-top: 0; margin-bottom: 14px;">
        <span>${escapeHtml(auth.error)}</span>
      </div>`
    : "";

  const closeBtnHtml = !isProtectedLock
    ? `<button type="button" class="banner-close" data-action="close-auth-modal" aria-label="Close">&times;</button>`
    : "";

  const cancelBtnHtml = !isProtectedLock
    ? `<button type="button" class="btn btn-secondary" data-action="close-auth-modal">${escapeHtml(cancelBtn)}</button>`
    : "";

  return `<div class="modal-backdrop" id="auth-modal-backdrop">
  <div class="modal-card card" role="dialog" aria-modal="true" aria-labelledby="auth-modal-title">
    <div class="card-header">
      <h2 class="card-title" id="auth-modal-title">${escapeHtml(title)}</h2>
      ${closeBtnHtml}
    </div>
    <p class="card-subtitle" style="margin-bottom: 16px;">${escapeHtml(desc)}</p>
    ${errorBanner}
    <form data-form="auth-login">
      <div class="form-group">
        <label class="form-label" for="auth-login-password">${escapeHtml(passwordLabel)}</label>
        <input id="auth-login-password" type="password" name="password" class="form-control" required autofocus placeholder="••••••••">
      </div>
      <div style="display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px;">
        ${cancelBtnHtml}
        <button type="submit" class="btn btn-primary">${escapeHtml(loginBtn)}</button>
      </div>
    </form>
  </div>
</div>`;
}

/**
 * Renders the topbar authentication action button (Login in public mode, Logout when authenticated).
 *
 * @param {object} state - Application state
 * @param {object} [opts]
 * @param {(key: string, vars?: Record<string, string | number>) => any} [opts.t] - Translator function
 * @returns {string} HTML string
 */
export function renderAuthStatusBadge(state, { t = (k) => k } = {}) {
  const auth = state?.auth || { mode: "none", authenticated: false };

  if (auth.mode === "none" && !auth.authenticated) {
    return "";
  }

  if (auth.authenticated) {
    const logoutLabel = t("auth.logout") || "Sair";
    return `<button type="button" class="btn btn-secondary btn-sm" data-action="auth-logout">${escapeHtml(logoutLabel)}</button>`;
  }

  if (auth.mode === "public") {
    const loginLabel = t("auth.login") || "Entrar";
    return `<button type="button" class="btn btn-secondary btn-sm" data-action="open-auth-modal">${escapeHtml(loginLabel)}</button>`;
  }

  return "";
}

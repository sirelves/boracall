// BoraCall — REST API client
// Auto-resolves the backend URL. Persists JWT in localStorage.
// Exposes `window.api` with explicit methods per endpoint + a low-level request().

(function () {
  const TOKEN_KEY = "bc_token";
  const USER_KEY  = "bc_user";

  function baseUrl() {
    // Order of precedence:
    //   1. window.BC_API_URL (set by dist/env.js if present)
    //   2. <meta name="bc-api-url" content="..."> override
    //   3. default localhost
    if (typeof window.BC_API_URL === "string" && window.BC_API_URL) return window.BC_API_URL;
    const meta = document.querySelector('meta[name="bc-api-url"]');
    if (meta && meta.content) return meta.content;
    return "http://127.0.0.1:3030";
  }

  function getToken() {
    try { return localStorage.getItem(TOKEN_KEY) || null; } catch { return null; }
  }
  function setToken(t) {
    try {
      if (t) localStorage.setItem(TOKEN_KEY, t);
      else   localStorage.removeItem(TOKEN_KEY);
    } catch {}
  }
  function getUser() {
    try { return JSON.parse(localStorage.getItem(USER_KEY) || "null"); } catch { return null; }
  }
  function setUser(u) {
    try {
      if (u) localStorage.setItem(USER_KEY, JSON.stringify(u));
      else   localStorage.removeItem(USER_KEY);
    } catch {}
  }

  async function request(path, { method = "GET", body, auth = true, signal } = {}) {
    const headers = { "Accept": "application/json" };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    if (auth) {
      const t = getToken();
      if (t) headers["Authorization"] = "Bearer " + t;
    }
    let res;
    try {
      res = await fetch(baseUrl() + path, {
        method,
        headers,
        body: body !== undefined ? JSON.stringify(body) : undefined,
        signal,
      });
    } catch (e) {
      throw new ApiError(0, "network", e.message || "network error");
    }
    const ct = res.headers.get("Content-Type") || "";
    const payload = ct.includes("application/json") ? await res.json().catch(() => null) : await res.text().catch(() => "");
    if (!res.ok) {
      const err = (payload && payload.error) || "http_" + res.status;
      const msg = (payload && payload.message) || (typeof payload === "string" ? payload : res.statusText);
      throw new ApiError(res.status, err, msg);
    }
    return payload;
  }

  class ApiError extends Error {
    constructor(status, code, message) {
      super(message || code);
      this.status  = status;
      this.code    = code;
    }
  }

  // ----- Auth --------------------------------------------------------------
  async function signup({ email, password, displayName }) {
    const r = await request("/api/auth/signup", {
      method: "POST",
      auth: false,
      body: { email, password, display_name: displayName ?? null },
    });
    setToken(r.token); setUser(r.user);
    return r;
  }

  async function login({ email, password }) {
    const r = await request("/api/auth/login", {
      method: "POST", auth: false,
      body: { email, password },
    });
    setToken(r.token); setUser(r.user);
    return r;
  }

  async function requestOtp() {
    return request("/api/auth/request-otp", { method: "POST" });
  }

  async function verifyOtp(code) {
    const u = await request("/api/auth/verify-otp", {
      method: "POST", body: { code },
    });
    setUser(u);
    return u;
  }

  async function requestPasswordReset(email) {
    return request("/api/auth/request-password-reset", {
      method: "POST", auth: false,
      body: { email },
    });
  }

  async function resetPassword({ email, code, newPassword }) {
    const r = await request("/api/auth/reset-password", {
      method: "POST", auth: false,
      body: { email, code, new_password: newPassword },
    });
    setToken(r.token); setUser(r.user);
    return r;
  }

  async function me() {
    const u = await request("/api/auth/me");
    setUser(u);
    return u;
  }

  async function updateMe({ displayName }) {
    const u = await request("/api/auth/me", {
      method: "PATCH", body: { display_name: displayName },
    });
    setUser(u);
    return u;
  }

  function logout() { setToken(null); setUser(null); }

  // ----- Servidores e canais ----------------------------------------------
  function humanTime(iso) {
    if (!iso) return "—";
    const diff = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
    if (diff < 60)     return "agora";
    if (diff < 3600)   return Math.floor(diff / 60) + "m";
    if (diff < 86400)  return Math.floor(diff / 3600) + "h";
    if (diff < 604800) return Math.floor(diff / 86400) + "d";
    return "há muito";
  }

  const enc = encodeURIComponent;

  const listServers  = ()             => request("/api/servers");
  const createServer = (name)         => request("/api/servers", { method: "POST", body: { name } });
  const getServer    = (slug)         => request("/api/servers/" + enc(slug));
  const joinServer   = (slug)         => request("/api/servers/" + enc(slug) + "/join", { method: "POST" });
  const createChannel = (slug, { name, kind }) =>
    request("/api/servers/" + enc(slug) + "/channels", { method: "POST", body: { name, kind } });
  const getChannel   = (slug)         => request("/api/channels/" + enc(slug));

  // ----- Mensagens ---------------------------------------------------------
  function listMessages(channelSlug, { before, limit } = {}) {
    const qs = new URLSearchParams();
    if (before) qs.set("before", before);
    if (limit)  qs.set("limit", String(limit));
    const q = qs.toString();
    return request("/api/channels/" + enc(channelSlug) + "/messages" + (q ? "?" + q : ""));
  }
  const sendMessage = (channelSlug, body) =>
    request("/api/channels/" + enc(channelSlug) + "/messages", { method: "POST", body: { body } });
  const editMessage = (id, body) =>
    request("/api/messages/" + enc(id), { method: "PATCH", body: { body } });
  const deleteMessage = (id) =>
    request("/api/messages/" + enc(id), { method: "DELETE" });
  const markRead = (channelSlug, messageId) =>
    request("/api/channels/" + enc(channelSlug) + "/read", {
      method: "PUT", body: { message_id: messageId || null },
    });

  /// Aceita link completo, caminho ou slug puro: "boracall.com/c/ab3kz", "/c/ab3kz", "ab3kz".
  function parseChannelLink(input) {
    const t = String(input || "").trim();
    const m = /(?:\/c\/|\/s\/|^)([a-z0-9]{3,})\/?$/i.exec(t);
    return m ? m[1] : null;
  }

  // ----- WebRTC ------------------------------------------------------------
  // A credencial de TURN é efêmera, então a resposta é cacheada só por alguns
  // minutos — tempo suficiente pra não pedir a cada entrada em canal, curto o
  // bastante pra não usar credencial vencida.
  let _iceCache = null;
  async function iceServers({ force = false } = {}) {
    if (!force && _iceCache && Date.now() < _iceCache.validoAte) return _iceCache.valor;
    const r = await request("/api/ice");
    const valor = (r.ice_servers || []).map((s) => ({
      urls: s.urls,
      ...(s.username ? { username: s.username, credential: s.credential } : {}),
    }));
    const ttlMs = Math.max(60, (r.ttl || 3600) - 300) * 1000;
    _iceCache = { valor, validoAte: Date.now() + ttlMs };
    return valor;
  }

  // ----- System ------------------------------------------------------------
  const health  = ()  => request("/api/health", { auth: false });
  const version = ()  => request("/api/version", { auth: false });

  // Expose publicly
  window.api = {
    baseUrl,
    request,
    ApiError,
    // auth
    signup, login, requestOtp, verifyOtp, requestPasswordReset, resetPassword, me, updateMe, logout,
    getToken, getUser, setToken, setUser,
    // servidores e canais
    listServers, createServer, getServer, joinServer, createChannel, getChannel,
    // mensagens
    listMessages, sendMessage, editMessage, deleteMessage, markRead,
    // webrtc
    iceServers,
    // util
    parseChannelLink, humanTime,
    // system
    health, version,
  };
})();

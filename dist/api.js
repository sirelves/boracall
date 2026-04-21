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

  // ----- Rooms -------------------------------------------------------------
  // The server returns { id, slug, name, room_type, locked, created_by, created_at, last_active_at, live }
  // The UI historically expected { id, slug, name, type, live(bool), count, members[], speaking, lastActive(human) }.
  // Normalize here so the rest of the app keeps using one shape.
  function humanTime(iso) {
    if (!iso) return "—";
    const diff = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
    if (diff < 60)     return "agora";
    if (diff < 3600)   return Math.floor(diff / 60) + "m";
    if (diff < 86400)  return Math.floor(diff / 3600) + "h";
    if (diff < 604800) return Math.floor(diff / 86400) + "d";
    return "há muito";
  }
  function normalizeRoom(r) {
    if (!r) return r;
    return {
      id: r.id,
      slug: r.slug,
      name: r.name,
      type: r.room_type || r.type,
      locked: !!r.locked,
      created_by: r.created_by,
      created_at: r.created_at,
      last_active_at: r.last_active_at,
      lastActive: humanTime(r.last_active_at),
      count: r.live || 0,
      live: (r.live || 0) > 0,
      speaking: 0,
      members: [],  // fills from WebSocket presence once connected
    };
  }

  const listRooms  = async ()                   => (await request("/api/rooms")).map(normalizeRoom);
  const createRoom = async ({ name, type, pw }) => normalizeRoom(await request("/api/rooms", {
    method: "POST",
    body: { name, room_type: type, password: pw || null },
  }));
  const getRoom    = async (slug)               => normalizeRoom(await request("/api/rooms/" + encodeURIComponent(slug)));
  const joinRoom   = async (slug, password)     => normalizeRoom(await request(
    "/api/rooms/" + encodeURIComponent(slug) + "/join",
    { method: "POST", body: { password: password || null } },
  ));

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
    // rooms
    listRooms, createRoom, getRoom, joinRoom,
    // system
    health, version,
  };
})();

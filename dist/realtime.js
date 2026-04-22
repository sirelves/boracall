// BoraCall — WebSocket realtime (signaling) client.
// Thin event-emitter over a reconnecting WebSocket.
// Use: const rt = new window.Realtime(slug); rt.on("offer", ...); rt.connect();

(function () {
  // JWT travels as a WebSocket subprotocol — never as a query param, since
  // URLs leak into proxy logs and browser history. The server extracts the
  // token from Sec-WebSocket-Protocol and echoes back "bc.v1".
  const WS_PROTOCOL = "bc.v1";
  const WS_TOKEN_PREFIX = "token.";

  function wsUrl(slug) {
    const api = (window.api && window.api.baseUrl()) || "http://127.0.0.1:3030";
    const wsBase = api.replace(/^http/, "ws");
    return wsBase + "/ws/rooms/" + encodeURIComponent(slug);
  }

  class Realtime {
    constructor(slug, opts = {}) {
      this.slug    = slug;
      this.token   = opts.token || (window.api && window.api.getToken());
      this.ws      = null;
      this.state   = "idle";       // idle | connecting | open | closed | reconnecting
      this.retry   = 0;
      this.maxRetry = opts.maxRetry ?? 6;
      this.handlers = new Map();   // type -> Set<fn>
      this._pingTimer = null;
      this._closedIntentionally = false;
    }
    on(type, fn) {
      if (!this.handlers.has(type)) this.handlers.set(type, new Set());
      this.handlers.get(type).add(fn);
      return () => this.off(type, fn);
    }
    off(type, fn) {
      const set = this.handlers.get(type);
      if (set) set.delete(fn);
    }
    _emit(type, payload) {
      const set = this.handlers.get(type);
      if (set) for (const fn of set) { try { fn(payload); } catch (e) { console.error(e); } }
      const any = this.handlers.get("*");
      if (any) for (const fn of any) { try { fn(type, payload); } catch (e) { console.error(e); } }
    }
    connect() {
      if (!this.token) throw new Error("realtime: no auth token");
      this._closedIntentionally = false;
      this.state = this.retry ? "reconnecting" : "connecting";
      this._emit("_state", this.state);

      const ws = new WebSocket(wsUrl(this.slug, this.token));
      this.ws = ws;

      ws.addEventListener("open", () => {
        this.state = "open"; this.retry = 0;
        this._emit("_state", "open");
        this._startPing();
      });
      ws.addEventListener("message", (ev) => {
        let msg; try { msg = JSON.parse(ev.data); } catch { return; }
        if (msg && msg.type) this._emit(msg.type, msg);
      });
      ws.addEventListener("close", (ev) => {
        this._stopPing();
        this.state = "closed";
        this._emit("_state", "closed");
        this._emit("_close", { code: ev.code, reason: ev.reason });
        if (!this._closedIntentionally && this.retry < this.maxRetry) {
          const delay = Math.min(500 * 2 ** this.retry, 8000);
          this.retry += 1;
          setTimeout(() => this.connect(), delay);
        }
      });
      ws.addEventListener("error", (e) => this._emit("_error", e));
    }
    close() {
      this._closedIntentionally = true;
      this._stopPing();
      if (this.ws && this.ws.readyState <= 1) {
        try { this.send({ type: "leave" }); } catch {}
        this.ws.close();
      }
    }
    send(obj) {
      if (!this.ws || this.ws.readyState !== 1) return false;
      this.ws.send(JSON.stringify(obj));
      return true;
    }
    _startPing() {
      this._stopPing();
      this._pingTimer = setInterval(() => { this.send({ type: "ping" }); }, 20000);
    }
    _stopPing() {
      if (this._pingTimer) { clearInterval(this._pingTimer); this._pingTimer = null; }
    }

    // --- signaling shortcuts ---
    offer(to, sdp)      { return this.send({ type: "offer",    to, sdp }); }
    answer(to, sdp)     { return this.send({ type: "answer",   to, sdp }); }
    ice(to, candidate)  { return this.send({ type: "ice",      to, candidate }); }
    mute(muted)         { return this.send({ type: "mute",     muted }); }
    speaking(level)     { return this.send({ type: "speaking", level }); }
  }

  window.Realtime = Realtime;
})();

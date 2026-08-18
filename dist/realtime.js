// BoraCall — cliente WebSocket (signaling + eventos do servidor).
// Emissor de eventos fino sobre um WebSocket que reconecta sozinho.
//
//   const rt = new window.Realtime(serverSlug);
//   rt.on("message", m => ...);
//   rt.connect();
//   rt.joinVoice(channelId);

(function () {
  // O JWT viaja como subprotocol do WebSocket — nunca como query param, porque
  // URL vaza em log de proxy, histórico de browser e header Referer. O servidor
  // extrai o token do Sec-WebSocket-Protocol e devolve só "bc.v1" no handshake.
  const WS_PROTOCOL = "bc.v1";
  const WS_TOKEN_PREFIX = "token.";

  function wsUrl(serverSlug) {
    const api = (window.api && window.api.baseUrl()) || "http://127.0.0.1:3030";
    const wsBase = api.replace(/^http/, "ws");
    return wsBase + "/ws/servers/" + encodeURIComponent(serverSlug);
  }

  class Realtime {
    constructor(serverSlug, opts = {}) {
      this.serverSlug = serverSlug;
      this.token   = opts.token || (window.api && window.api.getToken());
      this.ws      = null;
      this.state   = "idle";       // idle | connecting | open | closed | reconnecting
      this.retry   = 0;
      this.maxRetry = opts.maxRetry ?? 6;
      this.handlers = new Map();   // tipo -> Set<fn>
      this._pingTimer = null;
      this._closedIntentionally = false;
      // Canal de voz atual. Guardado pra reentrar sozinho depois de reconectar —
      // sem isso, uma queda de rede tira a pessoa da call em silêncio.
      this._voiceChannelId = null;
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
      if (!this.token) throw new Error("realtime: sem token de autenticação");
      this._closedIntentionally = false;
      this.state = this.retry ? "reconnecting" : "connecting";
      this._emit("_state", this.state);

      // A lista de protocolos é o que carrega a credencial. Sem ela o servidor
      // recusa o upgrade com 401.
      const ws = new WebSocket(wsUrl(this.serverSlug), [
        WS_PROTOCOL,
        WS_TOKEN_PREFIX + this.token,
      ]);
      this.ws = ws;

      ws.addEventListener("open", () => {
        this.state = "open"; this.retry = 0;
        this._emit("_state", "open");
        this._startPing();
        // Reentra no canal de voz onde a pessoa estava antes da queda.
        if (this._voiceChannelId) {
          this.send({ type: "join_voice", channel_id: this._voiceChannelId });
        }
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
          this.state = "reconnecting";
          this._emit("_state", "reconnecting");
          setTimeout(() => this.connect(), delay);
        }
      });
      ws.addEventListener("error", (e) => this._emit("_error", e));
    }
    close() {
      this._closedIntentionally = true;
      this._voiceChannelId = null;
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

    // --- voz ---------------------------------------------------------------
    get voiceChannelId() { return this._voiceChannelId; }

    joinVoice(channelId) {
      this._voiceChannelId = channelId;
      return this.send({ type: "join_voice", channel_id: channelId });
    }
    leaveVoice() {
      this._voiceChannelId = null;
      return this.send({ type: "leave_voice" });
    }

    // --- atalhos de signaling ---------------------------------------------
    offer(to, sdp)      { return this.send({ type: "offer",    to, sdp }); }
    answer(to, sdp)     { return this.send({ type: "answer",   to, sdp }); }
    ice(to, candidate)  { return this.send({ type: "ice",      to, candidate }); }
    mute(muted)         { return this.send({ type: "mute",     muted }); }
    speaking(level)     { return this.send({ type: "speaking", level }); }
    typing(channelId)   { return this.send({ type: "typing",   channel_id: channelId }); }
  }

  window.Realtime = Realtime;
})();

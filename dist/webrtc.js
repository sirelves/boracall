// BoraCall — WebRTC mesh manager.
// Given a Realtime instance + local MediaStream, wires up RTCPeerConnections
// with every other participant in the room. One peer connection per remote user.
//
// Emits events via a tiny event emitter:
//   - "peers" (Map<userId, {pc, stream?, displayName?, muted}>)
//   - "track" ({userId, stream})
//   - "removed" ({userId})
//   - "level"  ({userId, level})   // local-side volume metering per peer
//   - "local-level" (number)       // local mic level 0..1
//
// Topology = mesh. Fine up to ~4 simultaneous talkers. Beyond that we want an SFU.

(function () {
  const DEFAULT_ICE = [
    { urls: ["stun:stun.l.google.com:19302", "stun:stun1.l.google.com:19302"] },
    // TURN servers go here in prod (coturn behind the VPS) — without them,
    // ~30% of peers on symmetric NATs won't connect. See HANDOFF.
  ];

  async function acquireMic({ echoCancellation = true, noiseSuppression = true, autoGainControl = true } = {}) {
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
      throw new Error("mic: getUserMedia not available");
    }
    return navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation, noiseSuppression, autoGainControl },
      video: false,
    });
  }

  function attachLevelMeter(stream, onLevel, intervalMs = 150) {
    const AC = window.AudioContext || window.webkitAudioContext;
    if (!AC || !stream) return () => {};
    const ctx = new AC();
    const src = ctx.createMediaStreamSource(stream);
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 512;
    src.connect(analyser);
    const buf = new Uint8Array(analyser.frequencyBinCount);
    const timer = setInterval(() => {
      analyser.getByteTimeDomainData(buf);
      let sum = 0;
      for (let i = 0; i < buf.length; i++) {
        const v = (buf[i] - 128) / 128;
        sum += v * v;
      }
      onLevel(Math.min(1, Math.sqrt(sum / buf.length) * 2));
    }, intervalMs);
    return () => {
      clearInterval(timer);
      try { src.disconnect(); analyser.disconnect(); ctx.close(); } catch {}
    };
  }

  class Mesh {
    constructor(realtime, selfId, opts = {}) {
      this.rt        = realtime;
      this.selfId    = selfId;
      this.ice       = opts.iceServers || DEFAULT_ICE;
      this.localStream = null;
      this.peers     = new Map();   // userId -> { pc, stream?, displayName?, muted, el? }
      this.handlers  = new Map();
      this._detachLocalLevel = null;

      // Wire realtime events
      this.rt.on("presence", (m) => this._onPresence(m));
      this.rt.on("joined",   (m) => this._onJoined(m));
      this.rt.on("left",     (m) => this._onLeft(m));
      this.rt.on("offer",    (m) => this._onOffer(m));
      this.rt.on("answer",   (m) => this._onAnswer(m));
      this.rt.on("ice",      (m) => this._onIce(m));
      this.rt.on("mute",     (m) => this._onMute(m));
      this.rt.on("speaking", (m) => this._onSpeaking(m));
    }
    on(t, fn) { if (!this.handlers.has(t)) this.handlers.set(t, new Set()); this.handlers.get(t).add(fn); return () => this.off(t, fn); }
    off(t, fn) { const s = this.handlers.get(t); if (s) s.delete(fn); }
    _emit(t, p) { const s = this.handlers.get(t); if (s) for (const fn of s) { try { fn(p); } catch (e) { console.error(e); } } }

    async start({ stream } = {}) {
      this.localStream = stream || await acquireMic();
      this._detachLocalLevel = attachLevelMeter(this.localStream, (lvl) => {
        this._emit("local-level", lvl);
        if (lvl > 0.06) this.rt.speaking(lvl);
      });
      return this.localStream;
    }
    stop() {
      if (this._detachLocalLevel) this._detachLocalLevel();
      this._detachLocalLevel = null;
      for (const [, p] of this.peers) {
        try { p.pc.close(); } catch {}
        if (p.el) try { p.el.remove(); } catch {}
      }
      this.peers.clear();
      if (this.localStream) {
        this.localStream.getTracks().forEach(t => t.stop());
        this.localStream = null;
      }
    }

    setMuted(muted) {
      if (!this.localStream) return;
      this.localStream.getAudioTracks().forEach(t => (t.enabled = !muted));
      this.rt.mute(muted);
    }

    _ensurePeer(userId, displayName) {
      if (this.peers.has(userId)) return this.peers.get(userId);
      const pc = new RTCPeerConnection({ iceServers: this.ice });
      const peer = { pc, stream: null, displayName: displayName || null, muted: false };
      this.peers.set(userId, peer);

      // Pipe our local tracks
      if (this.localStream) {
        for (const track of this.localStream.getTracks()) pc.addTrack(track, this.localStream);
      }
      pc.addEventListener("track", (ev) => {
        peer.stream = ev.streams[0];
        // Attach to hidden audio element to actually play sound.
        let el = peer.el;
        if (!el) {
          el = document.createElement("audio");
          el.autoplay = true;
          el.playsInline = true;
          el.setAttribute("data-bc-peer", userId);
          document.body.appendChild(el);
          peer.el = el;
        }
        el.srcObject = peer.stream;
        this._emit("track", { userId, stream: peer.stream });
        this._emit("peers", new Map(this.peers));
      });
      pc.addEventListener("icecandidate", (ev) => {
        if (ev.candidate) this.rt.ice(userId, ev.candidate.toJSON ? ev.candidate.toJSON() : ev.candidate);
      });
      pc.addEventListener("connectionstatechange", () => {
        if (["failed", "closed", "disconnected"].includes(pc.connectionState)) {
          this._cleanupPeer(userId);
        }
      });
      this._emit("peers", new Map(this.peers));
      return peer;
    }

    _cleanupPeer(userId) {
      const p = this.peers.get(userId);
      if (!p) return;
      try { p.pc.close(); } catch {}
      if (p.el) try { p.el.remove(); } catch {}
      this.peers.delete(userId);
      this._emit("removed", { userId });
      this._emit("peers", new Map(this.peers));
    }

    async _onPresence(m) {
      // Initial snapshot — initiate offer to every peer with a lower-id tiebreaker,
      // so only one side creates the offer per pair (deterministic glare avoidance).
      for (const peer of m.peers || []) {
        if (peer.user_id === this.selfId) continue;
        this._ensurePeer(peer.user_id, peer.display_name);
        if (String(this.selfId) < String(peer.user_id)) {
          await this._createOffer(peer.user_id);
        }
      }
    }
    async _onJoined(m) {
      if (!m.peer || m.peer.user_id === this.selfId) return;
      this._ensurePeer(m.peer.user_id, m.peer.display_name);
      if (String(this.selfId) < String(m.peer.user_id)) {
        await this._createOffer(m.peer.user_id);
      }
    }
    _onLeft(m) {
      if (!m.user_id) return;
      this._cleanupPeer(m.user_id);
    }
    async _onOffer(m) {
      const peer = this._ensurePeer(m.from);
      await peer.pc.setRemoteDescription({ type: "offer", sdp: m.sdp });
      const answer = await peer.pc.createAnswer();
      await peer.pc.setLocalDescription(answer);
      this.rt.answer(m.from, answer.sdp);
    }
    async _onAnswer(m) {
      const peer = this.peers.get(m.from);
      if (!peer) return;
      await peer.pc.setRemoteDescription({ type: "answer", sdp: m.sdp });
    }
    async _onIce(m) {
      const peer = this.peers.get(m.from);
      if (!peer) return;
      try { await peer.pc.addIceCandidate(m.candidate); } catch (e) { /* late candidate after close — ignore */ }
    }
    _onMute(m) {
      const peer = this.peers.get(m.user_id);
      if (!peer) return;
      peer.muted = !!m.muted;
      this._emit("peers", new Map(this.peers));
    }
    _onSpeaking(m) {
      this._emit("level", { userId: m.user_id, level: m.level });
    }
    async _createOffer(userId) {
      const peer = this._ensurePeer(userId);
      const offer = await peer.pc.createOffer();
      await peer.pc.setLocalDescription(offer);
      this.rt.offer(userId, offer.sdp);
    }
  }

  window.WebRTCMesh = {
    Mesh,
    acquireMic,
    attachLevelMeter,
  };
})();

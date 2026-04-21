// BoraCall — screens part 2: create, join, invite-join, pre-call, call, post-call, settings
const { useState: useS2, useEffect: useE2, useRef: useR2, useMemo: useM2, useCallback: useC2 } = React;

// ---------- Create room modal ----------
function CreateRoom({ go, onCreate, activeRoom }) {
  const T = STRINGS.pt;
  const [name, setName] = useS2("");
  const [type, setType] = useS2("ephemeral");
  const [pw, setPw] = useS2("");
  const [busy, setBusy] = useS2(false);
  const valid = name.trim().length >= 2;

  // The backend mints the slug. This preview is illustrative only — the real slug
  // is known after /api/rooms returns.
  const publicHost = (window.BC_PUBLIC_URL || "https://boracall.com").replace(/^https?:\/\//, "");
  const previewLink = `${publicHost}/s/***`;
  const err = activeRoom && activeRoom._error;

  const submit = async () => {
    if (!valid || busy) return;
    setBusy(true);
    await onCreate({ name: name.trim(), type, pw });
    setBusy(false);
    // onCreate is responsible for transitioning; if it failed, activeRoom._error was set.
  };

  return (
    <div className="modal-overlay" onClick={()=>go("dashboard")}>
      <div className="modal" onClick={(e)=>e.stopPropagation()}>
        <div className="modal-head">
          <h3>{T.create_title}</h3>
          <button className="x" onClick={()=>go("dashboard")}>×</button>
        </div>
        <div className="modal-body">
          <div>
            <div className="label">{T.room_name}</div>
            <input className="input" placeholder={T.room_name_ph} value={name} onChange={(e)=>setName(e.target.value)} autoFocus />
            <div className="link-preview" style={{display:"flex",alignItems:"center",gap:8,marginTop:6}}>
              <span className="mono dim" style={{fontSize:11,flex:1,overflow:"hidden",textOverflow:"ellipsis",whiteSpace:"nowrap"}}>{previewLink}</span>
              <span className="mono dim" style={{fontSize:10}}>slug gerado ao criar</span>
            </div>
          </div>

          <div>
            <div className="label">{T.room_type}</div>
            <div className="seg" style={{marginTop:6}}>
              <button className={`seg-btn ${type==="ephemeral"?"seg-on":""}`} onClick={()=>setType("ephemeral")}>rápida</button>
              <button className={`seg-btn ${type==="persistent"?"seg-on":""}`} onClick={()=>setType("persistent")}>fixa</button>
            </div>
            <div className="dim" style={{fontSize:12,marginTop:6}}>
              {type === "ephemeral" ? T.ephemeral : T.persistent}
            </div>
          </div>

          <div>
            <div className="label">{T.room_password}</div>
            <input className="input" type="text" placeholder="em branco = livre" value={pw} onChange={(e)=>setPw(e.target.value)} />
            <div className="dim" style={{fontSize:11,marginTop:4,fontFamily:"var(--mono)"}}>{T.password_hint}</div>
          </div>

          {err && <div className="form-error mono">{err}</div>}
        </div>
        <div className="modal-foot">
          <button className="btn-ghost" onClick={()=>go("dashboard")}>{T.cancel}</button>
          <button className="btn-primary" disabled={!valid || busy} onClick={submit}>{busy ? "criando..." : T.create_cta}</button>
        </div>
      </div>
    </div>
  );
}

// ---------- Join by link ----------
function JoinByLink({ go, onPasteLink }) {
  const T = STRINGS.pt;
  const [link, setLink] = useS2("");
  const valid = /boracall\.app\/s\/[a-z0-9-]+/i.test(link);
  return (
    <div className="modal-overlay" onClick={()=>go("dashboard")}>
      <div className="modal" onClick={(e)=>e.stopPropagation()}>
        <div className="modal-head">
          <h3>{T.join_title}</h3>
          <button className="x" onClick={()=>go("dashboard")}>×</button>
        </div>
        <div className="modal-body">
          <div className="label">{T.paste_link}</div>
          <input className="input mono" placeholder={T.paste_ph} value={link} onChange={(e)=>setLink(e.target.value)} autoFocus />
          <div className="dim mono" style={{fontSize:11}}>Cola o link que te mandaram. A gente cuida do resto.</div>
        </div>
        <div className="modal-foot">
          <button className="btn-ghost" onClick={()=>go("dashboard")}>{T.cancel}</button>
          <button className="btn-primary" disabled={!valid} onClick={()=>{ onPasteLink(link); }}>{T.join_cta}</button>
        </div>
      </div>
    </div>
  );
}

// ---------- Invite landing (coming from a shared link) ----------
function InviteLanding({ go, invite, onAccept }) {
  const T = STRINGS.pt;
  const [pw, setPw] = useS2("");
  const needsPw = invite.locked;
  const canEnter = !needsPw || pw.length >= 1;

  return (
    <div className="invite-join">
      <div className="invite-top">
        <span className="host-av">{invite.hostInitials}</span>
        <div className="host-meta">
          <div className="t">{invite.room}</div>
          <div className="s">{T.invite_by} <b style={{color:"var(--fg)"}}>{invite.host}</b> · {(window.BC_PUBLIC_URL || "https://boracall.com").replace(/^https?:\/\//, "")}/s/{invite.slug}</div>
        </div>
      </div>
      <div className="invite-body">
        <div className="label">{T.invite_users_here}</div>
        <div className="mini-call">
          {invite.present.map((p, i) => (
            <div key={i} className={`mrow ${p.live?"live":""}`}>
              <span className="mdot" />
              <span>{p.name}</span>
              <span style={{marginLeft:"auto",color:"var(--fg-mute)"}}>{p.live?"LIVE":"IDLE"}</span>
            </div>
          ))}
        </div>
        {needsPw && (
          <>
            <div className="label" style={{color:"var(--bad)"}}>🔒 {T.invite_locked}</div>
            <input className="input" type="password" placeholder={T.room_password_ph} value={pw} onChange={(e)=>setPw(e.target.value)} />
          </>
        )}
        <button className="btn-primary" disabled={!canEnter} onClick={async ()=>{ await onAccept(needsPw ? pw : undefined); }}>{T.enter_room}</button>
        <div className="auth-foot" style={{margin:"0 -24px -20px",padding:"14px 24px",borderTop:"1px solid var(--line)"}}>
          <a onClick={()=>go("dashboard")}>{T.back}</a>
          <span className="dim">boracall</span>
        </div>
      </div>
    </div>
  );
}

// ---------- Pre-call (mic check) ----------
function PreCall({ go, activeRoom, session, tweaks, setTweak }) {
  const T = STRINGS.pt;
  const [level, setLevel] = useS2(0);
  const [device, setDevice] = useS2("padrão do sistema");
  const [startMuted, setStartMuted] = useS2(false);
  const [err, setErr] = useS2("");
  const streamRef = useR2(null);
  const detachRef = useR2(null);

  // Acquire a real mic stream for the VU meter. Release when leaving this screen.
  useE2(() => {
    let cancelled = false;
    (async () => {
      try {
        const stream = await window.WebRTCMesh.acquireMic();
        if (cancelled) { stream.getTracks().forEach(t=>t.stop()); return; }
        streamRef.current = stream;
        detachRef.current = window.WebRTCMesh.attachLevelMeter(stream, (l)=>setLevel(l), 120);
        // Figure out which device was picked.
        const track = stream.getAudioTracks()[0];
        if (track && track.label) setDevice(track.label);
      } catch (e) {
        setErr("microfone bloqueado — libera nas preferências do sistema");
      }
    })();
    return () => {
      cancelled = true;
      if (detachRef.current) detachRef.current();
      if (streamRef.current) streamRef.current.getTracks().forEach(t=>t.stop());
      streamRef.current = null;
      detachRef.current = null;
    };
  }, []);

  const enter = () => {
    // Hand the stream off to Call via a window global so we don't re-prompt.
    window.__bcPreferences = { startMuted };
    if (streamRef.current) {
      window.__bcLocalStream = streamRef.current;
      streamRef.current = null;
      if (detachRef.current) { detachRef.current(); detachRef.current = null; }
    }
    go("call");
  };

  return (
    <div className="auth" style={{maxWidth: 460}}>
      <div className="auth-head">
        <h2>{T.pre_title}</h2>
        <div className="sub">{activeRoom?.name || "sala"} · {T.pre_sub}</div>
      </div>
      <div className="auth-body">
        <div className="mic-check">
          <div className="mc-head"><span>{T.mic_level}</span><span className="dim">{T.test_mic}</span></div>
          <MicBars level={level} n={18} />
          <div className="mc-device"><span>{T.device_label}</span><b>{device}</b></div>
        </div>

        <div className="set-row" style={{padding:0,gridTemplateColumns:"1fr auto"}}>
          <div className="k">{T.start_muted} <span className="d">entrar com o microfone já desligado</span></div>
          <div className="seg">
            <button className={`seg-btn ${!startMuted?"seg-on":""}`} onClick={()=>setStartMuted(false)}>off</button>
            <button className={`seg-btn ${startMuted?"seg-on":""}`} onClick={()=>setStartMuted(true)}>on</button>
          </div>
        </div>

        <div className="set-row" style={{padding:0,gridTemplateColumns:"1fr auto"}}>
          <div className="k">Modo do microfone <span className="d">mantém ligado ou segura pra falar</span></div>
          <div className="seg">
            <button className={`seg-btn ${tweaks.micMode==="toggle"?"seg-on":""}`} onClick={()=>setTweak("micMode","toggle")}>toggle</button>
            <button className={`seg-btn ${tweaks.micMode==="ptt"?"seg-on":""}`} onClick={()=>setTweak("micMode","ptt")}>ptt</button>
          </div>
        </div>

        {err && <div className="form-error mono">{err}</div>}

        <button className="btn-primary" disabled={!!err} onClick={enter}>{T.join_call}</button>
      </div>
      <div className="auth-foot">
        <a onClick={()=>go("dashboard")}>{T.back}</a>
        <span className="dim mono">ready</span>
      </div>
    </div>
  );
}

// ---------- Call screen (real WebRTC mesh + signaling) ----------
function Call({ go, activeRoom, session, tweaks, onEnd }) {
  const T = STRINGS.pt;
  // Local-first user row; remote rows flow in from the mesh.
  const [peers, setPeers] = useS2([]);          // [{id, name, state, level, muted}]
  const [muted, setMuted] = useS2(!!(window.__bcPreferences && window.__bcPreferences.startMuted));
  const [ptt, setPtt] = useS2(false);
  const [elapsed, setElapsed] = useS2(0);
  const [uiHidden, setUiHidden] = useS2(false);
  const [status, setStatus] = useS2("connecting");    // connecting | live | reconnecting | closed | failed
  const [err, setErr] = useS2("");
  const [localLevel, setLocalLevel] = useS2(0);
  const [netStats, setNetStats] = useS2({ ping: 0, loss: 0 });

  const rtRef = useR2(null);
  const meshRef = useR2(null);
  const peakUsersRef = useR2(1);
  const speakersRef = useR2(new Map());          // userId -> { name, talkedSec }

  // --- Boot: wire up mesh + signaling --------------------------------
  useE2(() => {
    let disposed = false;
    (async () => {
      if (!activeRoom || !activeRoom.slug) { setErr("sala inválida"); return; }

      // Acquire mic — prefer the stream handed over from PreCall.
      let stream = window.__bcLocalStream;
      window.__bcLocalStream = null;
      try {
        if (!stream) stream = await window.WebRTCMesh.acquireMic();
      } catch (e) {
        setErr("microfone bloqueado"); return;
      }
      if (disposed) { stream.getTracks().forEach(t=>t.stop()); return; }

      if (muted) stream.getAudioTracks().forEach(t => (t.enabled = false));

      const rt = new window.Realtime(activeRoom.slug);
      rtRef.current = rt;
      const mesh = new window.WebRTCMesh.Mesh(rt, session.id || "me");
      meshRef.current = mesh;

      rt.on("_state", (s) => {
        if (s === "open") setStatus("live");
        else if (s === "reconnecting") setStatus("reconnecting");
        else if (s === "closed") setStatus("closed");
      });
      rt.on("_error", () => setStatus("failed"));

      // IMPORTANT: never store "me" in peers state — it'd capture stale `muted`
      // from closure. We derive the self row at render time from current state.
      const render = () => {
        const list = [];
        for (const [uid, p] of mesh.peers) {
          list.push({
            id: uid,
            name: p.displayName || "peer",
            state: p.muted ? "muted" : "listening",
            level: 0,
            muted: !!p.muted,
          });
        }
        if (list.length + 1 > peakUsersRef.current) peakUsersRef.current = list.length + 1;
        setPeers(list);
      };
      mesh.on("peers", render);
      mesh.on("local-level", (l) => setLocalLevel(l));
      mesh.on("level", ({ userId, level }) => {
        // Update speaking row by user id.
        setPeers(prev => prev.map(p => p.id === userId ? { ...p, state: level > 0.05 ? "speaking" : "listening", level } : p));
        const s = speakersRef.current.get(userId) || { name: "peer", talkedSec: 0 };
        s.talkedSec += 0.15; // ~150ms tick heuristic; precise stats come from WebRTC stats API later
        speakersRef.current.set(userId, s);
      });

      try {
        await mesh.start({ stream });
        rt.connect();
      } catch (e) {
        setErr("falha iniciando chamada: " + (e.message || e));
        return;
      }
      render();
    })();

    return () => {
      disposed = true;
      try { rtRef.current && rtRef.current.close(); } catch {}
      try { meshRef.current && meshRef.current.stop(); } catch {}
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Push mute state to mesh + broadcast.
  useE2(() => {
    if (meshRef.current) meshRef.current.setMuted(muted);
  }, [muted]);

  // --- Periodic WebRTC stats (real ping + loss from first remote peer) --
  useE2(() => {
    const iv = setInterval(async () => {
      const mesh = meshRef.current;
      if (!mesh) return;
      const first = mesh.peers.values().next().value;
      if (!first || !first.pc) return;
      try {
        const stats = await first.pc.getStats(null);
        let rttMs = null, loss = null;
        stats.forEach(r => {
          if (r.type === "candidate-pair" && r.state === "succeeded" && r.currentRoundTripTime != null) {
            rttMs = Math.round(r.currentRoundTripTime * 1000);
          }
          if (r.type === "inbound-rtp" && r.kind === "audio") {
            const lost = r.packetsLost || 0;
            const recv = r.packetsReceived || 1;
            loss = Math.max(0, Math.min(100, (lost / Math.max(1, lost + recv)) * 100));
          }
        });
        if (rttMs != null || loss != null) setNetStats(s => ({ ping: rttMs ?? s.ping, loss: loss ?? s.loss }));
      } catch {}
    }, 1500);
    return () => clearInterval(iv);
  }, []);

  // Elapsed clock.
  useE2(() => { const iv = setInterval(() => setElapsed(s => s+1), 1000); return () => clearInterval(iv); }, []);

  // Idle UI fade (only when invisible tweak on).
  const idleRef = useR2(null);
  useE2(() => {
    const bump = () => {
      setUiHidden(false);
      clearTimeout(idleRef.current);
      if (tweaks.invisible) idleRef.current = setTimeout(() => setUiHidden(true), 3500);
    };
    bump();
    window.addEventListener("mousemove", bump);
    window.addEventListener("keydown", bump);
    return () => {
      window.removeEventListener("mousemove", bump);
      window.removeEventListener("keydown", bump);
      clearTimeout(idleRef.current);
    };
  }, [tweaks.invisible]);

  const end = () => {
    // Hand real stats to onEnd.
    const speakers = Array.from(speakersRef.current.entries()).map(([id, s]) => ({
      name: s.name, talkedSec: Math.round(s.talkedSec), pct: Math.min(100, Math.round(s.talkedSec / Math.max(1, elapsed) * 100)),
    }));
    onEnd({
      peakUsers: peakUsersRef.current,
      avgPing:  netStats.ping,
      avgLoss:  netStats.loss,
      speakers,
    });
  };

  useE2(() => {
    const down = e => {
      if (e.target && e.target.tagName === "INPUT") return;
      if (e.code === "KeyM") { setMuted(m=>!m); e.preventDefault(); }
      if (e.code === "KeyH") setUiHidden(v=>!v);
      if (e.code === "Escape") end();
      if (e.code === "Space" && tweaks.micMode === "ptt" && !e.repeat) { setPtt(true); setMuted(false); e.preventDefault(); }
    };
    const up = e => {
      if (e.code === "Space" && tweaks.micMode === "ptt") { setPtt(false); setMuted(true); }
    };
    window.addEventListener("keydown", down); window.addEventListener("keyup", up);
    return () => { window.removeEventListener("keydown", down); window.removeEventListener("keyup", up); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tweaks.micMode]);

  // Self row is derived here (no stale closure): reflects live `muted` + `localLevel`.
  const selfRow = {
    id: session.id || "me",
    name: (session.displayName || session.email || "você") + "",
    state: muted ? "muted" : (localLevel > 0.08 ? "speaking" : "listening"),
    level: localLevel,
    isYou: true,
    muted,
  };
  const users = [selfRow, ...peers];
  const speakingCount = users.filter(u => u.state === "speaking").length;
  const mm = String(Math.floor(elapsed/60)).padStart(2,"0");
  const ss = String(elapsed%60).padStart(2,"0");
  const ordered = users;
  const statusLabel =
    status === "live" ? "Conectado" :
    status === "reconnecting" ? "Reconectando" :
    status === "failed" ? "Falhou" :
    status === "closed" ? "Desconectado" : "Conectando";
  const statusState =
    status === "live" ? "connected" :
    status === "failed" || status === "closed" ? "disconnected" : "connecting";
  const ping = netStats.ping;
  const loss = netStats.loss;

  return (
    <div className={`${uiHidden?"fade":""} ${tweaks.density}`} style={{width:"100%",display:"flex",justifyContent:"center"}} data-screen-label="11 Call Room">
      {ptt && tweaks.micMode === "ptt" && <div className="transmit">● TRANSMITINDO</div>}
      <main className="call" style={{opacity: uiHidden?0.05:1, transition:"opacity 200ms linear"}}>
        <header className="head">
          <div className="brand-row">
            <Mark size={14} />
            <span className="bn"><b>bora</b>call</span>
            <span className="room-crumb"><span className="slash">/</span>{activeRoom?.slug || "—"}<span className="slash">·</span><span className="mono">{mm}:{ss}</span></span>
          </div>
          <div style={{display:"flex",alignItems:"center",gap:10}}>
            <CopyButton text={`${window.BC_PUBLIC_URL || "https://boracall.com"}/s/${activeRoom?.slug}`} label="convite" done="link copiado!" />
            <Status state={statusState} label={statusLabel} />
          </div>
        </header>
        {err && <div className="form-error mono" style={{margin:"8px 0"}}>{err}</div>}
        <div className="meta">
          <div className="meta-count">
            <b>{users.length}</b> <span className="dim">{users.length === 1 ? "usuário" : "usuários"}</span>
            <span className="sep" style={{margin:"0 8px"}}>·</span>
            <b>{speakingCount}</b> <span className="dim">falando</span>
          </div>
          <PingMeter ping={Math.round(ping)} loss={loss} />
        </div>
        <div className="list">
          {ordered.map(u => <UserRow key={u.id} user={u} isYou={!!u.isYou} />)}
          {users.length <= 1 && (
            <div className="empty" style={{marginTop:12}}>Ninguém mais na sala. Compartilha o link pra chamar alguém.</div>
          )}
        </div>
        <div className="foot">
          {tweaks.micMode === "ptt" ? (
            <button className={`fbtn fbtn-ptt ${ptt?"ptt-active":""}`}
              onMouseDown={()=>{ setPtt(true); setMuted(false); }}
              onMouseUp={()=>{ setPtt(false); setMuted(true); }}
              onMouseLeave={()=>{ setPtt(false); setMuted(true); }}>
              <span className="dotmic" /><span>Segure pra falar</span><kbd>SPACE</kbd>
            </button>
          ) : (
            <button className={`fbtn ${muted?"fbtn-mute-on":""}`} onClick={()=>setMuted(m=>!m)}>
              <span className="dotmic" /><span>{muted?"Ativar mic":"Silenciar"}</span><kbd>M</kbd>
            </button>
          )}
          <button className="fbtn fbtn-leave" onClick={end}>
            <span>Sair</span><kbd>ESC</kbd>
          </button>
        </div>
        <div className="keys">
          <span className="keys-label">Atalhos</span>
          <div className="keys-group">
            <span><kbd>M</kbd> <span className="dim">mute</span></span>
            <span><kbd>SPACE</kbd> <span className="dim">ptt</span></span>
            <span><kbd>H</kbd> <span className="dim">ocultar</span></span>
            <span><kbd>ESC</kbd> <span className="dim">sair</span></span>
          </div>
        </div>
      </main>
    </div>
  );
}

// ---------- Post-call summary ----------
function PostCall({ go, activeRoom, callStats, onRate }) {
  const T = STRINGS.pt;
  const [rate, setRate] = useS2(null);
  const d = callStats || { duration: 0, peakUsers: 1, avgPing: 0, avgLoss: 0 };
  const speakers = (d.speakers && d.speakers.length) ? d.speakers : [];
  const mm = Math.floor(d.duration/60); const ss = d.duration%60;
  return (
    <div className="post">
      <div className="post-head">
        <div>
          <h2>{T.post_title}</h2>
          <div className="dim" style={{marginTop:4}}>{activeRoom?.name || "—"}</div>
        </div>
        <div className="when mono">há alguns segundos</div>
      </div>
      <div className="post-grid">
        <div className="stat"><div className="k">{T.duration}</div><div className="v">{mm}<span className="sub">m</span>{String(ss).padStart(2,"0")}<span className="sub">s</span></div></div>
        <div className="stat"><div className="k">{T.peak_users}</div><div className="v">{d.peakUsers}<span className="sub">pessoas</span></div></div>
        <div className="stat"><div className="k">{T.avg_ping}</div><div className="v">{d.avgPing}<span className="sub">ms</span></div></div>
        <div className="stat"><div className="k">{T.avg_loss}</div><div className="v">{d.avgLoss.toFixed(1)}<span className="sub">%</span></div></div>
      </div>
      <div className="post-body">
        <div className="label">{T.participants}</div>
        <div>
          {speakers.map((s, i) => (
            <div key={i} className="attendee">
              <span className="initials sm">{s.name.split(" ").map(x=>x[0]).join("").slice(0,2).toUpperCase()}</span>
              <div style={{flex:1}}>
                <div style={{fontSize:13}}>{s.name}</div>
                <div className="quality-bar" style={{marginTop:6}}>
                  <div className="quality-fill" style={{width: `${s.pct}%`}} />
                </div>
              </div>
              <span className="t">{s.talkedSec}s falando</span>
            </div>
          ))}
        </div>

        <div>
          <div className="label" style={{marginBottom:6}}>{T.rate_call}</div>
          <div className="rate-row">
            {[1,2,3,4,5].map(n => (
              <button key={n} className={`rate-btn ${rate>=n?"on":""}`} onClick={()=>setRate(n)}>{n}</button>
            ))}
            <span className="dim mono" style={{marginLeft:8}}>{rate?(rate<=2?T.rate_bad:rate<=3?T.rate_ok:T.rate_good):"—"}</span>
          </div>
        </div>

        <div style={{display:"flex",gap:8,marginTop:8}}>
          <button className="btn-line" onClick={()=>go("dashboard")}>{T.back_to_rooms}</button>
          <button className="btn-primary" onClick={()=>go("call")}>{T.call_again}</button>
        </div>
      </div>
    </div>
  );
}
// ---------- Settings ----------
function Settings({ go, session, setSession, tweaks, setTweak }) {
  const T = STRINGS.pt;
  const [tab, setTab] = useS2("profile");
  const [name, setName] = useS2(session.displayName || "Você");

  return (
    <div className="settings">
      <div className="appbar">
        <div className="brand-row">
          <Mark size={14} />
          <span className="bn"><b>bora</b>call</span>
          <span className="room-crumb"><span className="slash">/</span>configurações</span>
        </div>
        <button className="btn-ghost" onClick={()=>go("dashboard")}>← voltar</button>
      </div>
      <div className="set-tabs">
        {[["profile",T.tab_profile],["audio",T.tab_audio],["shortcuts",T.tab_shortcuts],["preferences","preferências"],["account",T.tab_account]].map(([k,l])=>(
          <button key={k} className={`set-tab ${tab===k?"on":""}`} onClick={()=>setTab(k)}>{l}</button>
        ))}
      </div>
      <div className="set-body">
        {tab === "profile" && (
          <>
            <div className="set-row">
              <div className="k">Nome <span className="d">aparece pros outros na call</span></div>
              <input className="input" value={name} onChange={(e)=>setName(e.target.value)} />
              <span />
            </div>
            <div className="set-row">
              <div className="k">E-mail <span className="d">usado pra login</span></div>
              <div className="mono dim">{session.email || "voce@empresa.com"}</div>
              <button className="btn-line">alterar</button>
            </div>
            <div className="set-row">
              <div className="k">Foto <span className="d">opcional — iniciais por padrão</span></div>
              <div className="row gap-3">
                <span className="initials lg">{name.split(" ").map(s=>s[0]).join("").slice(0,2).toUpperCase()}</span>
                <span className="dim">Sem foto · usa iniciais</span>
              </div>
              <button className="btn-line">upload</button>
            </div>
            <div style={{display:"flex",justifyContent:"flex-end",gap:8,marginTop:8}}>
              <button className="btn-ghost" onClick={()=>go("dashboard")}>{T.cancel}</button>
              <button className="btn-primary" onClick={()=>{ setSession(s=>({...s,displayName:name})); go("dashboard"); }}>{T.save}</button>
            </div>
          </>
        )}
        {tab === "audio" && (
          <>
            <div className="set-row">
              <div className="k">{T.input_device}</div>
              <div className="mono">MacBook Pro Mic</div>
              <button className="btn-line">mudar</button>
            </div>
            <div className="set-row">
              <div className="k">{T.output_device}</div>
              <div className="mono">AirPods Pro</div>
              <button className="btn-line">mudar</button>
            </div>
            <AudioToggle label={T.noise_sup} hint="remove ruídos de fundo (ventilador, teclado)" />
            <AudioToggle label={T.echo_can} hint="cancela o eco do alto-falante" defaultOn />
            <AudioToggle label={T.auto_gain} hint="normaliza o volume do microfone" />
            <AudioToggle label="Modo PTT por padrão" hint="entra nas calls com push-to-talk" />
          </>
        )}
        {tab === "shortcuts" && (
          <>
            <ShortcutRow k="M" d="Alternar mute" />
            <ShortcutRow k="SPACE" d="Push-to-talk (segurar)" />
            <ShortcutRow k="H" d="Ocultar/mostrar interface" />
            <ShortcutRow k="ESC" d="Sair da call" />
            <ShortcutRow k="?" d="Mostrar atalhos" />
            <ShortcutRow k="CMD + K" d="Buscar sala ou pessoa" />
          </>
        )}
        {tab === "preferences" && (
          <TweaksPanel tweaks={tweaks} setTweak={setTweak} />
        )}
        {tab === "account" && (
          <>
            <div className="set-row">
              <div className="k">Plano <span className="d">até 20 pessoas por sala</span></div>
              <div className="mono">Grátis</div>
              <button className="btn-line">upgrade</button>
            </div>
            <div className="set-row">
              <div className="k">Sessões ativas <span className="d">dispositivos conectados agora</span></div>
              <div className="mono">2 dispositivos</div>
              <button className="btn-line">ver</button>
            </div>
            <div className="divider" style={{margin:"10px 0"}} />
            <div className="set-row">
              <div className="k" style={{color:"var(--fg-dim)"}}>{T.sign_out}</div>
              <span />
              <button className="btn-line" onClick={()=>{
                try { window.api.logout(); } catch {}
                setSession({ email: "", displayName: "", id: null });
                go("landing");
              }}>sair</button>
            </div>
            <div className="set-row">
              <div className="k" style={{color:"var(--bad)"}}>{T.delete_acc} <span className="d">remove permanentemente</span></div>
              <span />
              <button className="btn-line" style={{borderColor:"var(--bad)",color:"var(--bad)"}}>apagar</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function ShortcutRow({ k, d }) {
  return (
    <div className="set-row">
      <div className="k">{d}</div>
      <span />
      <kbd>{k}</kbd>
    </div>
  );
}

function AudioToggle({ label, hint, defaultOn }) {
  const [on, setOn] = useS2(!!defaultOn);
  return (
    <div className="set-row">
      <div className="k">{label} <span className="d">{hint}</span></div>
      <span className="dim mono">{on?"ativo":"desligado"}</span>
      <div className="seg">
        <button className={`seg-btn ${!on?"seg-on":""}`} onClick={()=>setOn(false)}>off</button>
        <button className={`seg-btn ${on?"seg-on":""}`} onClick={()=>setOn(true)}>on</button>
      </div>
    </div>
  );
}

Object.assign(window, { CreateRoom, JoinByLink, InviteLanding, PreCall, Call, PostCall, Settings });

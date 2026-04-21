// BoraCall — screens part 1: landing, auth, otp, onboarding, dashboard
const { useState: useS1, useEffect: useE1, useRef: useR1, useMemo: useM1, useCallback: useC1 } = React;

// ---------- Landing ----------
function Landing({ go, variant = "A" }) {
  const T = STRINGS.pt;
  const [tick, setTick] = useS1(0);
  useE1(() => {
    const iv = setInterval(() => setTick(t => t + 1), 900);
    return () => clearInterval(iv);
  }, []);
  const demoUsers = [
    { n: "João P.", s: tick % 3 === 0 },
    { n: "Maria C.", s: tick % 3 === 1 },
    { n: "Lucas F.", s: tick % 3 === 2 },
    { n: "Ana B.", s: false },
  ];

  // variant B: headline emphasis differs; variant C: vertical layout
  const isC = variant === "C";

  return (
    <div className={`landing ${isC?"landing-c":""}`}>
      <div className="landing-left">
        <div style={{display:"flex",justifyContent:"space-between",alignItems:"center"}}>
          <Brand size={16} />
          <span className="pill"><span className="dot" />beta pública</span>
        </div>

        <div>
          {variant === "A" && (
            <h1>{T.tagline_1} <span className="amb">{T.tagline_2}</span></h1>
          )}
          {variant === "B" && (
            <h1>Cria sala. Compartilha link. <span className="amb">Bora falar.</span></h1>
          )}
          {variant === "C" && (
            <h1>Áudio em grupo <span className="amb">que some</span> quando você esquece dele.</h1>
          )}
          <p className="lede" style={{marginTop:14}}>{T.lede}</p>

          <div className="kvs">
            <span>{T.kv_latency}</span><b>{T.kv_latency_v}</b>
            <span>{T.kv_codec}</span><b>{T.kv_codec_v}</b>
            <span>{T.kv_cost}</span><b>{T.kv_cost_v}</b>
          </div>
        </div>

        <div className="actions">
          <button className="btn-primary" onClick={() => go("signup")}>{T.cta_start}</button>
          <button className="btn-line" onClick={() => go("login")}>{T.cta_have}</button>
        </div>
      </div>

      <div className="landing-right">
        <div className="demo-head">
          <span>{T.demo_label}</span>
          <span>42ms · 0.1%</span>
        </div>
        {demoUsers.map((u, i) => (
          <div key={i} className={`demo-row ${u.s?"on":""}`}>
            <span className="initials sm">{u.n.split(" ").map(s=>s[0]).join("")}</span>
            <span>{u.n}</span>
            <span className="tag">{u.s?"LIVE":"IDLE"}</span>
            <span className="dbars"><span className="db"/><span className="db"/><span className="db"/></span>
          </div>
        ))}
        <div style={{display:"flex",gap:8,marginTop:12}}>
          <button className="btn-line" style={{flex:1,padding:"10px"}}>🎤 mute</button>
          <button className="btn-ghost" style={{flex:1,padding:"10px",border:"1px solid var(--line)"}}>sair</button>
        </div>
      </div>
    </div>
  );
}

// ---------- Signup ----------
function Signup({ go, setSession }) {
  const T = STRINGS.pt;
  const [email, setEmail] = useS1("");
  const [pw, setPw] = useS1("");
  const [show, setShow] = useS1(false);
  const [err, setErr] = useS1("");
  const [busy, setBusy] = useS1(false);

  const valid = /.+@.+\..+/.test(email) && pw.length >= 8;

  return (
    <div className="auth">
      <div className="auth-head">
        <h2>{T.signup_title}</h2>
        <div className="sub">{T.signup_sub}</div>
      </div>
      <div className="auth-body">
        <label className="label">{T.email}</label>
        <input className="input" placeholder={T.email_ph} value={email} onChange={(e)=>setEmail(e.target.value)} />
        <label className="label">{T.password}</label>
        <div className="pw">
          <input className="input" type={show?"text":"password"} placeholder={T.password_ph} value={pw} onChange={(e)=>setPw(e.target.value)} />
          <button className="eye" onClick={()=>setShow(s=>!s)}>{show?T.hide:T.show}</button>
        </div>
        {err && <div className="form-error mono">{err}</div>}
        <button className="btn-primary" disabled={!valid || busy} onClick={async ()=>{
          setErr(""); setBusy(true);
          try {
            const r = await window.api.signup({ email, password: pw });
            setSession(s => ({ ...s, email: r.user.email, id: r.user.id, displayName: r.user.display_name || "" }));
            go("otp");
          } catch (e) { setErr(prettyApiErr(e)); }
          finally { setBusy(false); }
        }}>{busy ? "criando..." : T.create_acc}</button>
        <div className="dim mono" style={{fontSize:11,textAlign:"center"}}>{T.terms}</div>
      </div>
      <div className="auth-foot">
        <span>{T.has_account}</span>
        <a onClick={()=>go("login")}>{T.login_here}</a>
      </div>
    </div>
  );
}

// Helper reused across auth forms
function prettyApiErr(e) {
  if (!e) return "erro";
  if (e.code === "network") return "backend offline — confere se boracall-server tá rodando";
  if (e.code === "conflict") return "e-mail já cadastrado";
  if (e.code === "unauthorized") return "credenciais inválidas";
  if (e.code === "validation") return "dados inválidos";
  return e.message || "erro";
}

// ---------- Login ----------
function Login({ go, setSession }) {
  const T = STRINGS.pt;
  const [email, setEmail] = useS1("");
  const [pw, setPw] = useS1("");
  const [show, setShow] = useS1(false);
  const [err, setErr] = useS1("");
  const [busy, setBusy] = useS1(false);
  const valid = /.+@.+\..+/.test(email) && pw.length >= 1;
  return (
    <div className="auth">
      <div className="auth-head">
        <h2>{T.login_title}</h2>
        <div className="sub">{T.login_sub}</div>
      </div>
      <div className="auth-body">
        <label className="label">{T.email}</label>
        <input className="input" placeholder={T.email_ph} value={email} onChange={(e)=>setEmail(e.target.value)} />
        <label className="label">{T.password}</label>
        <div className="pw">
          <input className="input" type={show?"text":"password"} placeholder="••••••••" value={pw} onChange={(e)=>setPw(e.target.value)} />
          <button className="eye" onClick={()=>setShow(s=>!s)}>{show?T.hide:T.show}</button>
        </div>
        {err && <div className="form-error mono">{err}</div>}
        <button className="btn-primary" disabled={!valid || busy} onClick={async ()=>{
          setErr(""); setBusy(true);
          try {
            const r = await window.api.login({ email, password: pw });
            setSession(s => ({ ...s, email: r.user.email, id: r.user.id, displayName: r.user.display_name || "" }));
            go(r.user.display_name ? "dashboard" : "onboarding");
          } catch (e) { setErr(prettyApiErr(e)); }
          finally { setBusy(false); }
        }}>{busy ? "entrando..." : T.do_login}</button>
        <div style={{textAlign:"center",marginTop:4}}>
          <a className="dim" style={{cursor:"pointer",fontSize:12,borderBottom:"1px solid var(--line)"}} onClick={()=>go("forgot-password")}>esqueci minha senha</a>
        </div>
      </div>
      <div className="auth-foot">
        <span>{T.no_account}</span>
        <a onClick={()=>go("signup")}>{T.signup_here}</a>
      </div>
    </div>
  );
}

// ---------- Forgot password ----------
// Step 1: user digita email → server manda código 8 dígitos.
function ForgotPassword({ go, setSession }) {
  const [email, setEmail] = useS1("");
  const [err, setErr] = useS1("");
  const [busy, setBusy] = useS1(false);
  const valid = /.+@.+\..+/.test(email);

  const submit = async () => {
    if (!valid || busy) return;
    setErr(""); setBusy(true);
    try {
      await window.api.requestPasswordReset(email);
      setSession(s => ({ ...s, email }));  // carrega o e-mail pra próxima tela
      go("reset-password");
    } catch (e) { setErr(prettyApiErr(e)); }
    finally { setBusy(false); }
  };

  return (
    <div className="auth">
      <div className="auth-head">
        <h2>Redefinir senha</h2>
        <div className="sub">Digite seu e-mail. Se existir uma conta, mandamos um código.</div>
      </div>
      <div className="auth-body">
        <label className="label">e-mail</label>
        <input
          className="input"
          placeholder="voce@empresa.com"
          value={email}
          onChange={(e)=>setEmail(e.target.value)}
          onKeyDown={(e)=>{ if (e.key === "Enter") submit(); }}
          autoFocus
        />
        {err && <div className="form-error mono">{err}</div>}
        <button className="btn-primary" disabled={!valid || busy} onClick={submit}>
          {busy ? "enviando..." : "enviar código"}
        </button>
      </div>
      <div className="auth-foot">
        <a onClick={()=>go("login")}>← voltar pro login</a>
      </div>
    </div>
  );
}

// ---------- Reset password ----------
// Step 2: user digita código + nova senha → server troca a senha e loga.
function ResetPassword({ go, session, setSession }) {
  const [email, setEmail] = useS1(session.email || "");
  const [code, setCode] = useS1("");
  const [pw, setPw] = useS1("");
  const [show, setShow] = useS1(false);
  const [err, setErr] = useS1("");
  const [busy, setBusy] = useS1(false);
  const [sending, setSending] = useS1(false);
  const valid = /.+@.+\..+/.test(email) && /^\d{6,8}$/.test(code.trim()) && pw.length >= 8;

  const submit = async () => {
    if (!valid || busy) return;
    setErr(""); setBusy(true);
    try {
      const r = await window.api.resetPassword({ email, code: code.trim(), newPassword: pw });
      setSession(s => ({ ...s, email: r.user.email, id: r.user.id, displayName: r.user.display_name || "" }));
      go(r.user.display_name ? "dashboard" : "onboarding");
    } catch (e) { setErr(prettyApiErr(e)); }
    finally { setBusy(false); }
  };

  const resend = async () => {
    if (sending) return;
    setErr(""); setSending(true);
    try {
      await window.api.requestPasswordReset(email);
    } catch (e) { setErr(prettyApiErr(e)); }
    finally { setSending(false); }
  };

  return (
    <div className="auth">
      <div className="auth-head">
        <h2>Nova senha</h2>
        <div className="sub">Cola o código que chegou em <b>{session.email || "seu e-mail"}</b> e define uma senha nova.</div>
      </div>
      <div className="auth-body">
        {!session.email && (
          <>
            <label className="label">e-mail</label>
            <input className="input" value={email} onChange={(e)=>setEmail(e.target.value)} />
          </>
        )}
        <label className="label">código</label>
        <input
          className="input mono"
          placeholder="12345678"
          value={code}
          onChange={(e)=>setCode(e.target.value.replace(/\D/g,"").slice(0,8))}
          inputMode="numeric"
          autoFocus={!!session.email}
        />
        <label className="label">nova senha</label>
        <div className="pw">
          <input className="input" type={show?"text":"password"} placeholder="mínimo 8 caracteres" value={pw} onChange={(e)=>setPw(e.target.value)} />
          <button className="eye" onClick={()=>setShow(s=>!s)}>{show?"esconder":"mostrar"}</button>
        </div>
        {err && <div className="form-error mono">{err}</div>}
        <button className="btn-primary" disabled={!valid || busy} onClick={submit}>
          {busy ? "trocando..." : "redefinir senha"}
        </button>
        <div className="otp-resend mono">
          <a className="dim" style={{cursor:"pointer",borderBottom:"1px solid var(--line)"}} onClick={resend}>
            {sending ? "enviando..." : "reenviar código"}
          </a>
        </div>
      </div>
      <div className="auth-foot">
        <a onClick={()=>go("login")}>← voltar pro login</a>
      </div>
    </div>
  );
}

// Simple Google G glyph — geometric, not the real logo
function GoogleG({ size=16 }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" aria-hidden="true">
      <circle cx="8" cy="8" r="7" fill="none" stroke="currentColor" strokeWidth="1.25" />
      <rect x="8" y="7" width="5" height="2" fill="currentColor" />
    </svg>
  );
}

// ---------- OTP ----------
function OTP({ go, session }) {
  const T = STRINGS.pt;
  const [code, setCode] = useS1(["","","","","",""]);
  const [secs, setSecs] = useS1(28);
  const [err, setErr] = useS1("");
  const [sending, setSending] = useS1(false);
  const refs = useR1([]);
  const sentRef = useR1(false);

  useE1(() => {
    const iv = setInterval(() => setSecs(s => Math.max(0, s-1)), 1000);
    return () => clearInterval(iv);
  }, []);

  // Dispara o e-mail de OTP uma única vez ao entrar na tela.
  useE1(() => {
    if (sentRef.current) return;
    if (!window.api.getToken()) return;  // sem token não dá pra pedir (endpoint é autenticado)
    sentRef.current = true;
    setSending(true);
    window.api.requestOtp()
      .catch(e => setErr(prettyApiErr(e)))
      .finally(() => setSending(false));
  }, []);

  const submit = async (codeStr) => {
    try {
      await window.api.verifyOtp(codeStr);
      go("onboarding");
    } catch (e) { setErr(prettyApiErr(e)); }
  };

  const resend = async () => {
    if (sending || secs > 0) return;
    setErr(""); setSending(true);
    try {
      await window.api.requestOtp();
      setCode(["","","","","",""]);
      setSecs(28);
      refs.current[0]?.focus();
    } catch (e) { setErr(prettyApiErr(e)); }
    finally { setSending(false); }
  };

  const set = (i, v) => {
    v = v.replace(/\D/g,"").slice(0,1);
    const n = [...code]; n[i] = v; setCode(n);
    if (v && i < 5) refs.current[i+1]?.focus();
    if (n.every(x=>x)) setTimeout(()=>submit(n.join("")), 300);
  };
  const onKey = (i, e) => {
    if (e.key === "Backspace" && !code[i] && i > 0) refs.current[i-1]?.focus();
  };

  return (
    <div className="auth">
      <div className="auth-head">
        <h2>{T.otp_title}</h2>
        <div className="sub">{T.otp_sub} <b>{session.email || "voce@empresa.com"}</b></div>
      </div>
      <div className="auth-body">
        <div className="otp-boxes">
          {code.map((c, i) => (
            <input
              key={i}
              ref={el => refs.current[i] = el}
              className={`otp-input ${c?"filled":""}`}
              value={c}
              onChange={(e)=>set(i, e.target.value)}
              onKeyDown={(e)=>onKey(i,e)}
              inputMode="numeric"
              maxLength={1}
              autoFocus={i===0}
            />
          ))}
        </div>
        <div className="otp-resend mono">
          {sending
            ? "enviando..."
            : secs > 0
              ? `${T.otp_resend} ${secs}s`
              : <a className="dim" onClick={resend} style={{cursor:"pointer",borderBottom:"1px solid var(--line)"}}>{T.otp_resend_now}</a>}
        </div>
        {err && <div className="form-error mono">{err}</div>}
        <button className="btn-ghost" style={{alignSelf:"center"}} onClick={()=>go("signup")}>{T.otp_wrong}</button>
      </div>
    </div>
  );
}

// ---------- Onboarding (name + mic permission) ----------
function Onboarding({ go, session, setSession }) {
  const T = STRINGS.pt;
  const [name, setName] = useS1(session.displayName || "");
  const [micState, setMicState] = useS1("idle"); // idle | granted | denied | requesting
  const [level, setLevel] = useS1(0);
  const [err, setErr] = useS1("");
  const [busy, setBusy] = useS1(false);
  const streamRef = useR1(null);
  const detachRef = useR1(null);

  useE1(() => () => {
    // Cleanup on unmount
    if (detachRef.current) { try { detachRef.current(); } catch {} }
    if (streamRef.current) { try { streamRef.current.getTracks().forEach(t=>t.stop()); } catch {} }
  }, []);

  const requestMic = async () => {
    setErr(""); setMicState("requesting");
    try {
      const stream = await window.WebRTCMesh.acquireMic();
      streamRef.current = stream;
      detachRef.current = window.WebRTCMesh.attachLevelMeter(stream, (l)=>setLevel(l), 120);
      setMicState("granted");
    } catch (e) {
      setMicState("denied");
      setErr(e.message || "microfone bloqueado");
    }
  };

  const ready = name.trim().length >= 2 && micState === "granted";

  const finish = async () => {
    setErr(""); setBusy(true);
    try {
      const u = await window.api.updateMe({ displayName: name.trim() });
      setSession(s => ({ ...s, displayName: u.display_name, id: u.id, email: u.email }));
      // Free the preview mic — we'll re-request at Call-time.
      if (detachRef.current) { detachRef.current(); detachRef.current = null; }
      if (streamRef.current) { streamRef.current.getTracks().forEach(t=>t.stop()); streamRef.current = null; }
      go("dashboard");
    } catch (e) { setErr(prettyApiErr(e)); }
    finally { setBusy(false); }
  };

  return (
    <div className="auth" style={{maxWidth: 460}}>
      <div className="auth-head">
        <h2>{T.onb_title}</h2>
        <div className="sub">{T.onb_sub}</div>
      </div>
      <div className="auth-body">
        <label className="label">{T.display_name}</label>
        <input className="input" placeholder={T.display_name_ph} value={name} onChange={(e)=>setName(e.target.value)} />

        <div style={{marginTop:10}}>
          <div className="label">{T.mic_permission}</div>
          <div className="dim" style={{fontSize:12,marginTop:4,marginBottom:10}}>{T.mic_sub}</div>
          {(micState === "idle" || micState === "requesting") && (
            <button className="btn-line" style={{width:"100%"}} disabled={micState==="requesting"} onClick={requestMic}>
              <span className="dotmic" /> {micState === "requesting" ? "solicitando..." : T.mic_grant}
            </button>
          )}
          {micState === "denied" && (
            <div className="form-error mono" style={{margin:"6px 0"}}>microfone bloqueado — libera nas preferências do sistema e tenta de novo</div>
          )}
          {micState === "granted" && (
            <div className="mic-check">
              <div className="mc-head"><span>{T.mic_level}</span><span style={{color:"var(--accent)"}}>● {T.mic_granted}</span></div>
              <MicBars level={level} n={14} />
            </div>
          )}
        </div>
        {err && <div className="form-error mono">{err}</div>}
      </div>
      <div className="auth-foot">
        <a onClick={()=>go("landing")}>{T.back}</a>
        <button className="btn-primary" disabled={!ready || busy} onClick={finish}>{busy ? "salvando..." : T.onb_finish}</button>
      </div>
    </div>
  );
}

function MicBars({ level, n = 14 }) {
  const active = Math.round(level * n);
  return (
    <div className="mc-bars">
      {Array.from({length: n}).map((_, i) => {
        const h = 40 + (i % 5) * 12; // px scale — visual rhythm
        const on = i < active;
        return <span key={i} className={`mc-bar ${on?"on":""}`} style={{height: `${(i<active?60+Math.random()*40:15+i*1.2)}%`}} />;
      })}
    </div>
  );
}

// ---------- Dashboard (rooms) ----------
function Dashboard({ go, session, rooms, setActiveRoom, onCreate, onJoin, onPasteLink, loading }) {
  const T = STRINGS.pt;
  const [quick, setQuick] = useS1("");
  const persistent = rooms.filter(r => r.type === "persistent");
  const ephemeral = rooms.filter(r => r.type === "ephemeral");
  const liveCount = rooms.filter(r=>r.live).length;

  const doQuickJoin = () => {
    const v = quick.trim();
    if (!v) return;
    setQuick("");
    if (onPasteLink) onPasteLink(v);
  };

  return (
    <div className="dash" data-screen-label="06 Dashboard">
      <aside className="dash-side">
        <div className="sidetitle">Navegar</div>
        <div className="ws-item on">
          <SidebarIcon name="rooms" />
          <span>Salas</span>
        </div>
        <div className="ws-item" onClick={()=>go("settings")}>
          <SidebarIcon name="settings" />
          <span>{T.settings}</span>
        </div>
      </aside>

      <section className="dash-main">
        <div className="dash-head">
          <div>
            <h2>{T.your_rooms}</h2>
            <div className="sub">
              <b style={{color:liveCount>0?"var(--good)":"var(--fg-dim)"}}>{liveCount}</b> ao vivo
              {" · "}
              <b style={{color:"var(--fg)"}}>{rooms.length}</b> no total
              {" · entrada como "}
              <b style={{color:"var(--fg)"}}>{session.displayName || "você"}</b>
            </div>
          </div>
          <div className="dash-actions">
            <div className="quick-join">
              <input
                placeholder="cola link ou slug…"
                value={quick}
                onChange={e=>setQuick(e.target.value)}
                onKeyDown={e=>{ if (e.key === "Enter") doQuickJoin(); }}
              />
              <kbd>ENTER</kbd>
            </div>
            <button className="btn-line" onClick={()=>go("join")}>↳ {T.join_room}</button>
            <button className="btn-primary" onClick={()=>go("create")}>+ {T.create_room}</button>
          </div>
        </div>

        <div>
          <div className="label" style={{marginBottom:8}}>{T.rooms_persistent}</div>
          <div className="rooms-grid">
            {loading && persistent.length === 0 && <RoomCardSkeleton count={2} />}
            {!loading && persistent.length === 0 && (
              <div className="empty">Nenhuma sala fixa — salas fixas ficam ativas até você remover.</div>
            )}
            {persistent.map(r => <RoomCard key={r.id} r={r} onClick={()=>{ setActiveRoom(r); go(r.live?"call":"precall"); }} />)}
          </div>
        </div>

        <div>
          <div className="label" style={{marginBottom:8}}>{T.rooms_ephemeral}</div>
          <div className="rooms-grid">
            {loading && ephemeral.length === 0 && <RoomCardSkeleton count={3} />}
            {!loading && ephemeral.length === 0 && (
              <div className="empty">Nenhuma sala rápida ativa · crie uma e compartilhe o link</div>
            )}
            {ephemeral.map(r => <RoomCard key={r.id} r={r} onClick={()=>{ setActiveRoom(r); go(r.live?"call":"precall"); }} />)}
          </div>
        </div>
      </section>
    </div>
  );
}

function SidebarIcon({ name }) {
  const map = {
    rooms: <path d="M3 10h10M3 5h10M3 15h7" stroke="currentColor" strokeWidth="1.4" strokeLinecap="square" fill="none"/>,
    settings: <g fill="none" stroke="currentColor" strokeWidth="1.4"><circle cx="8" cy="8" r="2.5"/><path d="M8 1v2.2M8 12.8V15M1 8h2.2M12.8 8H15M3 3l1.6 1.6M11.4 11.4L13 13M3 13l1.6-1.6M11.4 4.6L13 3"/></g>,
  };
  return (
    <svg className="ws-icon" viewBox="0 0 16 16" aria-hidden="true">
      {map[name] || null}
    </svg>
  );
}

function RoomCardSkeleton({ count = 2 }) {
  return Array.from({length: count}).map((_,i)=>(
    <div key={i} className="room-card" style={{cursor:"default"}}>
      <div className="rc-top">
        <span className="skel skel-line" style={{width:"60%",height:16}} />
        <span className="skel skel-line" style={{width:40,height:10}} />
      </div>
      <div className="skel skel-line" style={{width:"40%",height:14}} />
      <div className="skel skel-line" style={{width:"30%",height:10}} />
    </div>
  ));
}

function RoomCard({ r, onClick }) {
  const inits = (n) => n.split(" ").map(s=>s[0]).join("").slice(0,2).toUpperCase();
  return (
    <div className={`room-card ${r.live?"live":""}`} onClick={onClick}>
      <div className="rc-top">
        <span className="rc-name">{r.name}</span>
        <span className={`rc-type ${r.live?"live":""}`}>
          {r.live ? (<><span className="live-dot" /> ao vivo</>) : (r.type === "persistent" ? "fixa" : "rápida")}
        </span>
      </div>
      <div className="rc-users">
        <span className="rc-stack">
          {r.members.slice(0,4).map((m,i)=>(<span key={i} className="initials sm">{inits(m)}</span>))}
        </span>
        <span>{r.count || r.members.length}</span>
      </div>
      <div className="rc-foot">
        <span>{r.live ? `${r.speaking||0} falando` : `ativa há ${r.lastActive}`}</span>
        <span className="mono">{r.slug}</span>
      </div>
      <span className="rc-enter">entrar →</span>
    </div>
  );
}

Object.assign(window, { Landing, Signup, Login, ForgotPassword, ResetPassword, OTP, Onboarding, Dashboard, RoomCard, RoomCardSkeleton, SidebarIcon, MicBars, GoogleG });

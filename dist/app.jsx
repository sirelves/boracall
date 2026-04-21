// BoraCall — main app + router + state
const { useState, useEffect, useMemo, useCallback, useRef } = React;

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "theme": "dark",
  "micMode": "toggle",
  "invisible": false,
  "density": "comfy",
  "accent": "amber",
  "device": "desktop",
  "landingVariant": "A"
}/*EDITMODE-END*/;

function persist(key, val) { try { localStorage.setItem(key, JSON.stringify(val)); } catch {} }
function load(key, def) { try { const v = localStorage.getItem(key); return v ? JSON.parse(v) : def; } catch { return def; } }

function App() {
  const [tweaks, setTweaks] = useState(() => ({ ...TWEAK_DEFAULTS, ...load("bc_tweaks", {}) }));
  // Default to landing only; we'll advance to dashboard once api.me() validates.
  const [route, setRoute] = useState(() => load("bc_route", "landing"));
  const [session, setSession] = useState(() => {
    const cached = (window.api && window.api.getUser()) || null;
    const legacy = load("bc_session", null);
    return cached || legacy || { email: "", displayName: "", id: null };
  });
  const [rooms, setRooms] = useState([]);
  const [activeRoom, setActiveRoom] = useState(null);
  const [showTweaks, setShowTweaks] = useState(false);
  const [callStats, setCallStats] = useState(null);
  const [callStart, setCallStart] = useState(null);
  const [invitePreview, setInvitePreview] = useState(null);
  const [bootDone, setBootDone] = useState(false);
  const mountedRef = useRef(false);

  useEffect(() => { document.documentElement.setAttribute("data-theme", tweaks.theme); document.documentElement.setAttribute("data-accent", tweaks.accent); persist("bc_tweaks", tweaks); }, [tweaks]);
  useEffect(() => persist("bc_route", route), [route]);
  useEffect(() => persist("bc_session", session), [session]);

  // Sync "invisible" tweak to the native window.
  useEffect(() => {
    if (window.desktop && window.desktop.window && window.desktop.isNative) {
      window.desktop.window.setInvisibleMode(!!tweaks.invisible).catch(() => {});
    }
  }, [tweaks.invisible]);

  // --- Boot: revalidate token, load rooms -------------------------------
  useEffect(() => {
    if (mountedRef.current) return;
    mountedRef.current = true;
    (async () => {
      // If we already have a token cached, try to refresh user.
      if (window.api && window.api.getToken()) {
        try {
          const u = await window.api.me();
          setSession(s => ({ ...s, id: u.id, email: u.email, displayName: u.display_name || "" }));
          // If we're on an auth screen but already logged in, bounce to dashboard.
          const authish = ["landing","signup","login","otp"].includes(route);
          if (authish && u.display_name) setRoute("dashboard");
          else if (authish && !u.display_name) setRoute("onboarding");
        } catch (e) {
          // Stale or rejected token — start fresh.
          window.api.logout();
          setSession({ email: "", displayName: "", id: null });
          setRoute("landing");
        }
      } else {
        // No token — route through auth.
        if (!["landing","signup","login"].includes(route)) setRoute("landing");
      }
      setBootDone(true);
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Refresh rooms when entering dashboard.
  const refreshRooms = useCallback(async () => {
    if (!window.api || !window.api.getToken()) return;
    try {
      const list = await window.api.listRooms();
      setRooms(list);
    } catch (e) { /* stay silent; dashboard will show empty */ }
  }, []);
  useEffect(() => {
    if (route === "dashboard" || route === "create" || route === "join") refreshRooms();
  }, [route, refreshRooms]);

  const go = useCallback((r) => setRoute(r), []);
  const setTweak = useCallback((k, v) => {
    setTweaks(prev => {
      const next = { ...prev, [k]: v };
      try { window.parent.postMessage({ type: "__edit_mode_set_keys", edits: { [k]: v } }, "*"); } catch {}
      return next;
    });
  }, []);

  useEffect(() => {
    const onMsg = (e) => {
      const d = e.data || {};
      if (d.type === "__activate_edit_mode") setShowTweaks(true);
      else if (d.type === "__deactivate_edit_mode") setShowTweaks(false);
    };
    window.addEventListener("message", onMsg);
    try { window.parent.postMessage({ type: "__edit_mode_available" }, "*"); } catch {}
    return () => window.removeEventListener("message", onMsg);
  }, []);

  // --- Create: talks to the real backend --------------------------------
  const onCreate = useCallback(async (data) => {
    try {
      const r = await window.api.createRoom({ name: data.name, type: data.type, pw: data.pw });
      setRooms(prev => [r, ...prev.filter(x => x.slug !== r.slug)]);
      setActiveRoom(r);
      setRoute("precall");
    } catch (e) {
      // Surface through activeRoom.error channel — Create screen shows toast.
      setActiveRoom({ _error: e.message || "criação falhou" });
    }
  }, []);

  // --- Join via pasted link: resolve slug and go to invite preview ------
  const onPasteLink = useCallback(async (link) => {
    // Accept "https://boracall.app/s/<slug>" or "/s/<slug>" or just "<slug>".
    const m = /(?:\/s\/|^)([a-z0-9-]{3,})$/i.exec((link || "").trim());
    const slug = m ? m[1] : link;
    try {
      const r = await window.api.getRoom(slug);
      setActiveRoom(r);
      setInvitePreview({
        room: r.name,
        slug: r.slug,
        host: "host",
        hostInitials: "H",
        locked: r.locked,
        present: [],
      });
      setRoute("invite");
    } catch (e) {
      setActiveRoom({ _error: e.message || "sala não encontrada" });
    }
  }, []);

  // --- Accept invite ----------------------------------------------------
  const onAcceptInvite = useCallback(async (password) => {
    if (!activeRoom || !activeRoom.slug) return;
    try {
      const r = await window.api.joinRoom(activeRoom.slug, password);
      setActiveRoom(r);
      setRoute("precall");
    } catch (e) {
      setActiveRoom(r => ({ ...r, _error: e.message || "não pôde entrar" }));
    }
  }, [activeRoom]);

  // --- Call lifecycle ---------------------------------------------------
  const onCallStart = useCallback(() => { setCallStart(Date.now()); setCallStats(null); }, []);
  const onCallEnd = useCallback((maybeStats) => {
    const dur = callStart ? Math.floor((Date.now() - callStart)/1000) : 0;
    setCallStats({
      duration: Math.max(1, dur),
      peakUsers: (maybeStats && maybeStats.peakUsers) || 1,
      avgPing:  (maybeStats && maybeStats.avgPing)  || 0,
      avgLoss:  (maybeStats && maybeStats.avgLoss)  || 0,
      speakers: (maybeStats && maybeStats.speakers) || [],
    });
    setRoute("postcall");
  }, [callStart]);

  useEffect(() => { if (route === "call") onCallStart(); }, [route, onCallStart]);

  const label = useMemo(() => {
    const T = STRINGS.pt;
    const map = {
      landing: T.r_landing, signup: T.r_signup, login: T.r_login, otp: T.r_otp,
      onboarding: T.r_onboarding, dashboard: T.r_dashboard, create: T.r_create,
      join: T.r_join, invite: T.r_invite, precall: T.r_precall, call: T.r_call,
      postcall: T.r_postcall, settings: T.r_settings,
    };
    return map[route] || route;
  }, [route]);

  const mobile = tweaks.device === "mobile";

  const screen = <Router
    route={route} go={go} session={session} setSession={setSession}
    rooms={rooms} activeRoom={activeRoom} setActiveRoom={setActiveRoom}
    onCreate={onCreate} onPasteLink={onPasteLink} onEnd={onCallEnd}
    onAcceptInvite={onAcceptInvite}
    invitePreview={invitePreview}
    tweaks={tweaks} setTweak={setTweak}
    callStats={callStats}
  />;

  return (
    <div className="shell" data-screen-label={`${mobile?"Mobile · ":""}${label}`}>
      {mobile ? <PhoneFrame>{screen}</PhoneFrame> : (
        <div className={route === "dashboard" || route === "settings" ? "page page-wide" : "page"}>
          {screen}
        </div>
      )}

      {!showTweaks ? (
        <button className="tweaks-fab" onClick={()=>setShowTweaks(true)}>Tweaks</button>
      ) : (
        <TweaksPanel tweaks={tweaks} setTweak={setTweak} onClose={()=>setShowTweaks(false)} />
      )}
    </div>
  );
}

// ----- Router -----
function Router(p) {
  switch (p.route) {
    case "landing":    return <Landing go={p.go} variant={p.tweaks.landingVariant} />;
    case "signup":     return <Signup go={p.go} setSession={p.setSession} />;
    case "login":      return <Login go={p.go} setSession={p.setSession} />;
    case "otp":        return <OTP go={p.go} session={p.session} />;
    case "onboarding": return <Onboarding go={p.go} session={p.session} setSession={p.setSession} />;
    case "dashboard":  return <Dashboard go={p.go} session={p.session} rooms={p.rooms} setActiveRoom={p.setActiveRoom} />;
    case "create":     return <DashBehind go={p.go} session={p.session} rooms={p.rooms} setActiveRoom={p.setActiveRoom}><CreateRoom go={p.go} onCreate={p.onCreate} activeRoom={p.activeRoom} /></DashBehind>;
    case "join":       return <DashBehind go={p.go} session={p.session} rooms={p.rooms} setActiveRoom={p.setActiveRoom}><JoinByLink go={p.go} onPasteLink={p.onPasteLink} activeRoom={p.activeRoom} /></DashBehind>;
    case "invite":     return <InviteLanding go={p.go} invite={p.invitePreview || { room: p.activeRoom?.name, slug: p.activeRoom?.slug, host: "host", hostInitials: "H", locked: !!p.activeRoom?.locked, present: [] }} onAccept={p.onAcceptInvite} />;
    case "precall":    return <PreCall go={p.go} activeRoom={p.activeRoom} session={p.session} tweaks={p.tweaks} setTweak={p.setTweak} />;
    case "call":       return <Call go={p.go} activeRoom={p.activeRoom} session={p.session} tweaks={p.tweaks} onEnd={p.onEnd} />;
    case "postcall":   return <PostCall go={p.go} activeRoom={p.activeRoom} callStats={p.callStats} />;
    case "settings":   return <Settings go={p.go} session={p.session} setSession={p.setSession} tweaks={p.tweaks} setTweak={p.setTweak} />;
    default:           return <Landing go={p.go} variant="A" />;
  }
}

function DashBehind({ children, ...rest }) {
  return (<>
    <Dashboard {...rest} />
    {children}
  </>);
}

const root = ReactDOM.createRoot(document.getElementById("root"));
root.render(<App />);

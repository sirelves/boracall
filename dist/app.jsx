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
  // Começa na landing; o boot avança pro app quando o api.me() valida a sessão.
  const [route, setRoute] = useState(() => load("bc_route", "landing"));
  const [session, setSession] = useState(() => {
    const cached = (window.api && window.api.getUser()) || null;
    const legacy = load("bc_session", null);
    return cached || legacy || { email: "", displayName: "", id: null };
  });
  const [showTweaks, setShowTweaks] = useState(false);
  const [bootDone, setBootDone] = useState(false);
  const [updateInfo, setUpdateInfo] = useState(null);  // { version, current_version, notes }
  const [updateState, setUpdateState] = useState("idle"); // idle | installing | error
  const [updateErr, setUpdateErr] = useState("");
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

  // Check for updates 5s após o boot — não bloqueia a tela inicial.
  useEffect(() => {
    if (!window.desktop || !window.desktop.isNative) return;
    const timer = setTimeout(async () => {
      try {
        const info = await window.desktop.updater.check();
        if (info && info.available) setUpdateInfo(info);
      } catch (e) {
        // Silencioso: endpoint 404, offline, etc. Retry na próxima abertura.
        console.warn("updater: check falhou", e);
      }
    }, 5000);
    return () => clearTimeout(timer);
  }, []);

  const installUpdate = useCallback(async () => {
    if (updateState === "installing") return;
    setUpdateState("installing"); setUpdateErr("");
    try {
      await window.desktop.updater.install();
      // Se chegou aqui sem reiniciar, algo estranho — mas o app deveria ter feito restart.
    } catch (e) {
      setUpdateState("error");
      setUpdateErr(e.message || String(e));
    }
  }, [updateState]);

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
          // Já logado numa tela de auth? Vai direto pro app.
          const authish = ["landing","signup","login","otp"].includes(route);
          if (authish && u.display_name) setRoute("app");
          else if (authish && !u.display_name) setRoute("onboarding");
        } catch (e) {
          // Stale or rejected token — start fresh.
          window.api.logout();
          setSession({ email: "", displayName: "", id: null });
          setRoute("landing");
        }
      } else {
        // No token — route through auth.
        if (!["landing","signup","login","forgot-password","reset-password"].includes(route)) setRoute("landing");
      }
      setBootDone(true);
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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

  const label = useMemo(() => {
    const T = STRINGS.pt;
    const map = {
      landing: T.r_landing, signup: T.r_signup, login: T.r_login, otp: T.r_otp,
      onboarding: T.r_onboarding, app: "servidores", settings: T.r_settings,
    };
    return map[route] || route;
  }, [route]);

  const mobile = tweaks.device === "mobile";

  const screen = <Router
    route={route} go={go} session={session} setSession={setSession}
    tweaks={tweaks} setTweak={setTweak}
  />;

  return (
    <div className="shell" data-screen-label={`${mobile?"Mobile · ":""}${label}`}>
      {updateInfo && (
        <div className="update-banner" role="status">
          <span className="mono">
            nova versão <b>{updateInfo.version}</b> disponível
            {updateState === "error" && updateErr ? ` · erro: ${updateErr}` : null}
          </span>
          <div style={{display:"flex",gap:8}}>
            <button
              className="btn-primary"
              disabled={updateState === "installing"}
              onClick={installUpdate}
            >
              {updateState === "installing" ? "baixando..." : "atualizar agora"}
            </button>
            <button
              className="btn-ghost"
              onClick={() => setUpdateInfo(null)}
              disabled={updateState === "installing"}
            >
              depois
            </button>
          </div>
        </div>
      )}

      {mobile ? <PhoneFrame>{screen}</PhoneFrame> : (
        <div className={["app","settings"].includes(route) ? "page page-wide" : "page"}>
          {screen}
        </div>
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
    case "forgot-password": return <ForgotPassword go={p.go} setSession={p.setSession} />;
    case "reset-password":  return <ResetPassword  go={p.go} session={p.session} setSession={p.setSession} />;
    case "otp":        return <OTP go={p.go} session={p.session} />;
    case "onboarding": return <Onboarding go={p.go} session={p.session} setSession={p.setSession} />;
    case "app":        return <AppShell go={p.go} session={p.session} setSession={p.setSession} tweaks={p.tweaks} setTweak={p.setTweak} />;
    case "settings":   return <Settings go={p.go} session={p.session} setSession={p.setSession} tweaks={p.tweaks} setTweak={p.setTweak} />;
    default:           return <Landing go={p.go} variant="A" />;
  }
}

const root = ReactDOM.createRoot(document.getElementById("root"));
root.render(<App />);

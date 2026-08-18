// BoraCall — shared components (Mark, Tweaks, Status, PingMeter, UserRow, etc.)
const { useState, useEffect, useRef, useMemo, useCallback } = React;

// ---------- i18n (PT-BR only for content; tweaks still offer EN) ----------
const STRINGS = {
  pt: {
    // shared
    back: "voltar", next: "continuar", cancel: "cancelar", copy: "copiar", copied: "copiado!",
    tweaks: "Tweaks",
    // landing
    tagline_1: "Teu grupo,", tagline_2: "sem peso.",
    lede: "Servidores com canais de texto e de voz. Nativo, leve, abre num piscar — o oposto de deixar um navegador inteiro aberto pra conversar.",
    kv_latency: "Latência", kv_latency_v: "< 50ms p50",
    kv_codec: "Codec", kv_codec_v: "Opus 48kHz",
    kv_cost: "Canais", kv_cost_v: "texto e voz",
    cta_start: "Criar conta grátis", cta_have: "Já tenho conta",
    demo_label: "🔊 Geral · agora",
    // auth
    signup_title: "Cria sua conta", signup_sub: "Leva 30 segundos. Promessa.",
    login_title: "Entra aí", login_sub: "Bom te ver de novo.",
    or: "ou",
    email: "E-mail", email_ph: "voce@empresa.com",
    password: "Senha", password_ph: "mínimo 8 caracteres",
    show: "mostrar", hide: "esconder",
    create_acc: "Criar conta", do_login: "Entrar",
    terms: "Ao criar conta você aceita os termos.",
    no_account: "Não tem conta?", has_account: "Já tem conta?",
    signup_here: "cadastra aqui", login_here: "entra aqui",
    // OTP
    otp_title: "Confirma o e-mail", otp_sub: "Mandamos um código de 6 dígitos pra",
    otp_wrong: "E-mail errado?", otp_resend: "Reenviar em",
    otp_resend_now: "reenviar agora",
    // Onboarding
    onb_title: "Como querem te chamar?", onb_sub: "Esse é o nome que vai aparecer pros outros.",
    display_name: "Nome de exibição", display_name_ph: "ex: João P.",
    mic_permission: "Libera o microfone", mic_sub: "A gente precisa pra você falar, óbvio.",
    mic_grant: "Permitir microfone", mic_granted: "Microfone ok",
    mic_denied: "Bloqueado pelo navegador",
    onb_finish: "Bora começar",
    // Dashboard
    workspace: "Workspace",
    rooms_persistent: "Salas fixas", rooms_ephemeral: "Salas rápidas",
    your_rooms: "Suas salas", create_room: "Criar sala", join_room: "Entrar por link",
    live_now: "ao vivo", idle: "vazia",
    last_active: "ativa há",
    // Create room
    create_title: "Nova sala",
    room_name: "Nome da sala", room_name_ph: "ex: bora-falar",
    room_type: "Tipo",
    ephemeral: "Rápida (some quando esvaziar)",
    persistent: "Fixa (fica no workspace)",
    room_password: "Senha (opcional)", password_hint: "Deixa em branco se qualquer um com o link pode entrar",
    create_cta: "Criar e entrar",
    // Join
    join_title: "Entrar por link",
    paste_link: "Cola o link do convite",
    paste_ph: "https://boracall.com/s/...",
    join_cta: "Entrar",
    // Invite screen (shown to joiners)
    invite_host: "Você foi convidado pra",
    invite_by: "por",
    invite_users_here: "pessoas já na sala",
    invite_locked: "Essa sala pede senha",
    room_password_ph: "Senha da sala",
    enter_room: "Entrar na sala",
    // Pre-call
    pre_title: "Pronto pra entrar?",
    pre_sub: "Confere o microfone antes.",
    test_mic: "Fale qualquer coisa pra testar",
    mic_level: "Nível do mic",
    device_label: "Dispositivo",
    join_call: "Entrar na call",
    start_muted: "Entrar mutado",
    // Post-call
    post_title: "Call encerrada",
    post_sub_pre: "Você ficou",
    duration: "Duração", peak_users: "Pico", avg_ping: "Ping médio", avg_loss: "Perda média",
    participants: "Participantes",
    rate_call: "Como foi a qualidade?",
    rate_bad: "ruim", rate_ok: "ok", rate_good: "ótima",
    back_to_rooms: "Voltar pras salas", call_again: "Ligar de novo",
    // Settings
    settings: "Configurações",
    tab_profile: "Perfil", tab_audio: "Áudio", tab_shortcuts: "Atalhos", tab_account: "Conta",
    save: "Salvar alterações",
    sign_out: "Sair da conta", delete_acc: "Apagar conta",
    input_device: "Entrada", output_device: "Saída",
    noise_sup: "Cancelamento de ruído", echo_can: "Cancelamento de eco",
    auto_gain: "Ganho automático",
    enabled: "ativo", disabled: "desligado",
    // Nav
    r_landing: "landing", r_signup: "cadastro", r_login: "login",
    r_otp: "otp", r_onboarding: "onboarding", r_dashboard: "salas",
    r_create: "criar", r_join: "entrar", r_invite: "convite",
    r_precall: "pré-call", r_call: "sala", r_postcall: "pós-call", r_settings: "config",
  },
};

// ---------- Clipboard copy helper (desktop-bridge aware) ----------
function CopyButton({ text, label = "copiar", done = "copiado!", style }) {
  const [state, setState] = useState("idle");
  const onClick = async (e) => {
    e.stopPropagation();
    try {
      if (window.desktop && window.desktop.clipboard) {
        await window.desktop.clipboard.writeText(text);
      } else {
        await navigator.clipboard.writeText(text);
      }
      setState("done");
      setTimeout(() => setState("idle"), 1400);
    } catch (err) {
      setState("err");
      setTimeout(() => setState("idle"), 1400);
    }
  };
  return (
    <button
      type="button"
      className={`copy-btn ${state}`}
      onClick={onClick}
      aria-label={label}
      style={style}
      title={state === "done" ? done : label}
    >
      {state === "done" ? "✓ " + done : state === "err" ? "× falhou" : "⎘ " + label}
    </button>
  );
}

// ---------- Logo mark ----------
function Mark({ size = 14, color = "currentColor" }) {
  return (
    <svg width={size} height={size} viewBox="0 0 14 14" aria-hidden="true" style={{ display: "block" }}>
      <rect x="0.5" y="0.5" width="13" height="13" fill="none" stroke={color} strokeWidth="1" />
      <rect x="3" y="3" width="3" height="8" fill={color} />
      <rect x="8" y="5" width="3" height="4" fill={color} />
    </svg>
  );
}

function Brand({ size = 14 }) {
  return (
    <span className="brand-row">
      <Mark size={size} />
      <span className="bn"><b>bora</b>call</span>
    </span>
  );
}

// ---------- Pulse status ----------
function Status({ state, label }) {
  return (
    <div className="status-s">
      <span className={`pulse pulse-${state}`} />
      <span className="mono up">{label}</span>
    </div>
  );
}

function PingMeter({ ping, loss }) {
  const tier = ping < 60 ? "good" : ping < 150 ? "mid" : "bad";
  return (
    <div className="meter">
      <span className={`mdot mdot-${tier}`} />
      <span className="dim">ping</span>
      <span>{ping}ms</span>
      <span className="sep">·</span>
      <span className="dim">loss</span>
      <span>{loss.toFixed(1)}%</span>
    </div>
  );
}

// ---------- Tweaks panel ----------
function TweaksPanel({ tweaks, setTweak, onClose }) {
  const Row = ({ label, children }) => (
    <div className="tw-row">
      <span className="tw-label mono dim">{label}</span>
      <div>{children}</div>
    </div>
  );
  const Seg = ({ value, options, onChange }) => (
    <div className="seg">
      {options.map((o) => (
        <button key={o.v} onClick={() => onChange(o.v)} className={`seg-btn ${value === o.v ? "seg-on" : ""}`}>
          {o.l}
        </button>
      ))}
    </div>
  );
  return (
    <div className="tweaks tweaks-embed">
      <Row label="Tema">
        <Seg value={tweaks.theme} options={[{v:"dark",l:"escuro"},{v:"light",l:"claro"}]} onChange={(v)=>setTweak("theme",v)} />
      </Row>
      <Row label="Acento">
        <div className="seg">
          {[{v:"amber",c:"oklch(0.82 0.17 75)"},{v:"green",c:"oklch(0.80 0.17 145)"},{v:"cyan",c:"oklch(0.80 0.13 220)"},{v:"mono",c:"oklch(0.78 0 0)"}].map((o)=>(
            <button key={o.v} onClick={()=>setTweak("accent",o.v)} className={`seg-btn ${tweaks.accent===o.v?"seg-on":""}`}>
              <span style={{display:"inline-block",width:10,height:10,background:o.c}} />
            </button>
          ))}
        </div>
      </Row>
      <Row label="Mic">
        <Seg value={tweaks.micMode} options={[{v:"toggle",l:"toggle"},{v:"ptt",l:"ptt"}]} onChange={(v)=>setTweak("micMode",v)} />
      </Row>
      <Row label="UI invis.">
        <Seg value={tweaks.invisible?"on":"off"} options={[{v:"off",l:"off"},{v:"on",l:"on"}]} onChange={(v)=>setTweak("invisible",v==="on")} />
      </Row>
      <Row label="Densidade">
        <Seg value={tweaks.density} options={[{v:"compact",l:"compacta"},{v:"comfy",l:"confort."}]} onChange={(v)=>setTweak("density",v)} />
      </Row>
      <Row label="Device">
        <Seg value={tweaks.device} options={[{v:"desktop",l:"desktop"},{v:"mobile",l:"mobile"}]} onChange={(v)=>setTweak("device",v)} />
      </Row>
      <Row label="Landing">
        <Seg value={tweaks.landingVariant} options={[{v:"A",l:"A"},{v:"B",l:"B"},{v:"C",l:"C"}]} onChange={(v)=>setTweak("landingVariant",v)} />
      </Row>
    </div>
  );
}

// ---------- User row (call) ----------
function UserRow({ user, isYou }) {
  const { name, state, level } = user;
  const speaking = state === "speaking";
  const muted = state === "muted";
  return (
    <div className={`urow ${speaking ? "urow-speak" : ""} ${muted ? "urow-mute" : ""}`}>
      <div className="urow-left">
        <span className="initials">{name.split(" ").map(s=>s[0]).slice(0,2).join("").toUpperCase()}</span>
        <span className="uname">{name}{isYou && <span className="you mono"> · você</span>}</span>
      </div>
      <div className="urow-right">
        <div className="bars" aria-label={speaking?"falando":muted?"mutado":"ouvindo"}>
          {[0,1,2,3,4].map(i=>(
            <span key={i} className="bar" style={{opacity: muted?0.08:(level>i/5?1:0.12)}} />
          ))}
        </div>
        <span className="state-tag mono">{muted?"MUTED":speaking?"LIVE":"IDLE"}</span>
      </div>
    </div>
  );
}

// ---------- Phone wrapper (mobile mock) ----------
function PhoneFrame({ children, time = "14:32" }) {
  return (
    <div className="phone-wrap">
      <div className="phone mobile">
        <div className="statusbar">
          <span>{time}</span>
          <span style={{display:"flex",gap:6,alignItems:"center"}}>
            <span className="mono">5G</span>
            <span style={{display:"inline-block",width:18,height:8,border:"1px solid currentColor"}}><span style={{display:"block",width:"70%",height:"100%",background:"currentColor"}} /></span>
          </span>
        </div>
        <div className="page">{children}</div>
      </div>
    </div>
  );
}

// ---------- Route stepper (top-center pill) ----------
const ROUTE_ORDER = ["landing","signup","login","otp","onboarding","dashboard","create","join","invite","precall","call","postcall","settings"];
function RouteStepper({ route, go }) {
  const T = STRINGS.pt;
  return (
    <nav className="route-stepper" aria-label="fluxo">
      {ROUTE_ORDER.map((r, i) => (
        <React.Fragment key={r}>
          {i > 0 && <span className="divi">·</span>}
          <button className={route===r?"on":""} onClick={()=>go(r)}>{T["r_"+r] || r}</button>
        </React.Fragment>
      ))}
    </nav>
  );
}

Object.assign(window, {
  STRINGS, Mark, Brand, Status, PingMeter, TweaksPanel, UserRow, PhoneFrame, RouteStepper, CopyButton,
});

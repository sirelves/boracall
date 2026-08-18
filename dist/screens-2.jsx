// BoraCall — telas do app autenticado: servidores, canais de texto e de voz.
//
// Layout, da esquerda pra direita:
//   trilha de servidores │ lista de canais │ conteúdo (chat ou call)
//
// Uma conexão WebSocket por servidor ativo, criada aqui e passada pra baixo.
// A mesh de voz vive dentro de um canal e é destruída ao sair dele.

const { useState: useS2, useEffect: useE2, useRef: useR2, useMemo: useM2, useCallback: useC2 } = React;

const iniciais = (nome) =>
  String(nome || "?")
    .split(/\s+/)
    .map((s) => s[0])
    .join("")
    .slice(0, 2)
    .toUpperCase();

const publicUrl = () =>
  (window.BC_PUBLIC_URL || "https://boracall.com").replace(/\/+$/, "");

// ---------------------------------------------------------------------------
// Casca do app — dona da conexão e do estado do servidor ativo
// ---------------------------------------------------------------------------

function AppShell({ go, session, setSession, tweaks, setTweak }) {
  const [servers, setServers] = useS2([]);
  const [activeSlug, setActiveSlug] = useS2(() => {
    try { return localStorage.getItem("bc_server") || null; } catch { return null; }
  });
  const [detail, setDetail] = useS2(null);           // servidor ativo + canais + membros
  const [activeChannelId, setActiveChannelId] = useS2(null);
  const [conn, setConn] = useS2("idle");             // idle | connecting | open | reconnecting | closed
  const [modal, setModal] = useS2(null);             // null | criar-servidor | criar-canal | convite | entrar
  const [erro, setErro] = useS2("");
  const [carregando, setCarregando] = useS2(true);

  // Voz: canal atual, mesh e pares. Vive acima do painel de conteúdo pra que
  // trocar de canal de texto não derrube a call.
  const [voiceChannelId, setVoiceChannelId] = useS2(null);
  const [voicePeers, setVoicePeers] = useS2([]);
  const [muted, setMuted] = useS2(false);
  const [voiceErro, setVoiceErro] = useS2("");

  const rtRef = useR2(null);
  const meshRef = useR2(null);

  const canais = detail?.channels || [];
  const canalAtivo = canais.find((c) => c.id === activeChannelId) || null;
  const canalDeVoz = canais.find((c) => c.id === voiceChannelId) || null;
  const souDono = detail?.role === "owner";

  // --- carregar lista de servidores ---------------------------------------
  const recarregarServidores = useC2(async () => {
    try {
      const list = await window.api.listServers();
      setServers(list);
      setActiveSlug((atual) => atual || (list[0] ? list[0].slug : null));
      return list;
    } catch (e) {
      setErro(e.message || "não deu pra carregar seus servidores");
      return [];
    } finally {
      setCarregando(false);
    }
  }, []);

  useE2(() => { recarregarServidores(); }, [recarregarServidores]);

  // --- carregar o servidor ativo ------------------------------------------
  const recarregarDetalhe = useC2(async (slug) => {
    if (!slug) { setDetail(null); return null; }
    try {
      const d = await window.api.getServer(slug);
      setDetail(d);
      setActiveChannelId((atual) => {
        const aindaExiste = d.channels.some((c) => c.id === atual);
        if (aindaExiste) return atual;
        const primeiroTexto = d.channels.find((c) => c.kind === "text");
        return primeiroTexto ? primeiroTexto.id : (d.channels[0]?.id ?? null);
      });
      return d;
    } catch (e) {
      setErro(e.message || "não deu pra abrir o servidor");
      return null;
    }
  }, []);

  useE2(() => {
    if (activeSlug) { try { localStorage.setItem("bc_server", activeSlug); } catch {} }
    recarregarDetalhe(activeSlug);
  }, [activeSlug, recarregarDetalhe]);

  // --- conexão WebSocket do servidor ativo --------------------------------
  useE2(() => {
    if (!activeSlug) return;
    // Trocar de servidor derruba a call: a mesh pertence ao canal de voz, que
    // pertence ao servidor que estamos deixando.
    pararVoz();

    const rt = new window.Realtime(activeSlug);
    rtRef.current = rt;

    rt.on("_state", (s) => setConn(s));
    rt.on("voice_state", (m) => {
      // Snapshot inicial: quantos em cada canal de voz.
      setDetail((d) => {
        if (!d) return d;
        const porCanal = new Map((m.channels || []).map((c) => [c.channel_id, c.peers.length]));
        return { ...d, channels: d.channels.map((c) => ({ ...c, live: porCanal.get(c.id) || 0 })) };
      });
    });
    const atualizaLive = (channelId, n) =>
      setDetail((d) => d && ({
        ...d,
        channels: d.channels.map((c) => (c.id === channelId ? { ...c, live: n } : c)),
      }));
    rt.on("voice_presence", (m) => atualizaLive(m.channel_id, (m.peers || []).length));
    rt.on("error", (m) => setVoiceErro(m.message || "erro no servidor"));

    rt.connect();
    return () => {
      pararVoz();
      try { rt.close(); } catch {}
      rtRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeSlug]);

  // --- voz -----------------------------------------------------------------
  function pararVoz() {
    if (meshRef.current) { try { meshRef.current.stop(); } catch {} meshRef.current = null; }
    if (rtRef.current && rtRef.current.voiceChannelId) {
      try { rtRef.current.leaveVoice(); } catch {}
    }
    setVoiceChannelId(null);
    setVoicePeers([]);
    setMuted(false);
  }

  const entrarNaVoz = useC2(async (canal) => {
    setVoiceErro("");
    const rt = rtRef.current;
    if (!rt) return;
    if (voiceChannelId === canal.id) return;
    if (meshRef.current) { try { meshRef.current.stop(); } catch {} meshRef.current = null; }

    let stream;
    try {
      stream = await window.WebRTCMesh.acquireMic();
    } catch (e) {
      setVoiceErro("microfone bloqueado — libera nas preferências do sistema");
      return;
    }

    const mesh = new window.WebRTCMesh.Mesh(rt, session.id, { channelId: canal.id });
    meshRef.current = mesh;
    mesh.on("peers", (map) => {
      setVoicePeers([...map.entries()].map(([id, p]) => ({
        id, name: p.displayName || "alguém", muted: !!p.muted, level: 0,
      })));
    });
    mesh.on("level", ({ userId, level }) =>
      setVoicePeers((prev) => prev.map((p) => (p.id === userId ? { ...p, level } : p))));

    await mesh.start({ stream });
    rt.joinVoice(canal.id);
    setVoiceChannelId(canal.id);
    setMuted(false);
  }, [session.id, voiceChannelId]);

  const sairDaVoz = useC2(() => pararVoz(), []);

  useE2(() => {
    if (meshRef.current) meshRef.current.setMuted(muted);
  }, [muted]);

  // Atalho global de mute enquanto estiver em call.
  useE2(() => {
    if (!voiceChannelId) return;
    const onKey = (e) => {
      if (e.target && ["INPUT", "TEXTAREA"].includes(e.target.tagName)) return;
      if (e.code === "KeyM") { setMuted((m) => !m); e.preventDefault(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [voiceChannelId]);

  // --- ações ---------------------------------------------------------------
  const criarServidor = async (nome) => {
    const s = await window.api.createServer(nome);
    await recarregarServidores();
    setActiveSlug(s.slug);
    setModal(null);
  };

  const criarCanal = async ({ nome, tipo }) => {
    await window.api.createChannel(activeSlug, { name: nome, kind: tipo });
    await recarregarDetalhe(activeSlug);
    setModal(null);
  };

  const entrarPorLink = async (link) => {
    const slug = window.api.parseChannelLink(link);
    if (!slug) throw new Error("link inválido");
    // O link é de canal: resolve, entra no servidor se precisar, abre o canal.
    const ch = await window.api.getChannel(slug);
    if (!ch.is_member) await window.api.joinServer(ch.server_slug);
    await recarregarServidores();
    setActiveSlug(ch.server_slug);
    setActiveChannelId(ch.id);
    setModal(null);
  };

  const zerarNaoLidas = useC2((channelId) => {
    setDetail((d) => d && ({
      ...d,
      channels: d.channels.map((c) => (c.id === channelId ? { ...c, unread: 0 } : c)),
    }));
  }, []);

  const incrementarNaoLidas = useC2((channelId) => {
    setDetail((d) => d && ({
      ...d,
      channels: d.channels.map((c) =>
        c.id === channelId ? { ...c, unread: (c.unread || 0) + 1 } : c),
    }));
  }, []);

  if (carregando) {
    return <div className="app-shell"><div className="empty" style={{margin:"auto"}}>carregando…</div></div>;
  }

  if (!servers.length) {
    return (
      <PrimeiroServidor
        go={go}
        onCriar={criarServidor}
        onEntrar={entrarPorLink}
        erro={erro}
      />
    );
  }

  return (
    <div className="app-shell">
      <ServerRail
        servers={servers}
        activeSlug={activeSlug}
        onPick={setActiveSlug}
        onCriar={() => setModal("criar-servidor")}
        onEntrar={() => setModal("entrar")}
        onSettings={() => go("settings")}
      />

      <ChannelList
        detail={detail}
        conn={conn}
        activeChannelId={activeChannelId}
        voiceChannelId={voiceChannelId}
        souDono={souDono}
        onPick={setActiveChannelId}
        onEntrarNaVoz={entrarNaVoz}
        onCriarCanal={() => setModal("criar-canal")}
        onConvidar={() => setModal("convite")}
        session={session}
      />

      <section className="conteudo">
        {voiceChannelId && (
          <VoiceBar
            canal={canalDeVoz}
            peers={voicePeers}
            muted={muted}
            setMuted={setMuted}
            onSair={sairDaVoz}
            session={session}
            erro={voiceErro}
          />
        )}
        {canalAtivo?.kind === "text" && (
          <TextChannel
            key={canalAtivo.id}
            canal={canalAtivo}
            rt={rtRef.current}
            session={session}
            onLido={zerarNaoLidas}
            onMensagemDeOutroCanal={incrementarNaoLidas}
          />
        )}
        {canalAtivo?.kind === "voice" && (
          <VoiceChannelPanel
            canal={canalAtivo}
            emCall={voiceChannelId === canalAtivo.id}
            peers={voicePeers}
            onEntrar={() => entrarNaVoz(canalAtivo)}
            onSair={sairDaVoz}
            erro={voiceErro}
          />
        )}
        {!canalAtivo && <div className="empty" style={{margin:"auto"}}>nenhum canal selecionado</div>}
      </section>

      {modal === "criar-servidor" && (
        <ModalSimples titulo="Novo servidor" rotulo="Nome do servidor"
          placeholder="Time Athmos" cta="criar" onFechar={() => setModal(null)} onEnviar={criarServidor} />
      )}
      {modal === "entrar" && (
        <ModalSimples titulo="Entrar por link" rotulo="Link do canal"
          placeholder={`${publicUrl().replace(/^https?:\/\//, "")}/c/ab3kz`} cta="entrar"
          mono onFechar={() => setModal(null)} onEnviar={entrarPorLink} />
      )}
      {modal === "criar-canal" && (
        <CriarCanal onFechar={() => setModal(null)} onEnviar={criarCanal} />
      )}
      {modal === "convite" && (
        <Convite detail={detail} onFechar={() => setModal(null)} />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Trilha de servidores
// ---------------------------------------------------------------------------

function ServerRail({ servers, activeSlug, onPick, onCriar, onEntrar, onSettings }) {
  return (
    <nav className="rail" aria-label="Servidores">
      {servers.map((s) => (
        <button
          key={s.id}
          className={`rail-item ${s.slug === activeSlug ? "on" : ""}`}
          title={s.name}
          onClick={() => onPick(s.slug)}
        >
          {iniciais(s.name)}
        </button>
      ))}
      <button className="rail-item rail-add" title="Criar servidor" onClick={onCriar}>+</button>
      <button className="rail-item rail-add" title="Entrar por link" onClick={onEntrar}>↳</button>
      <div style={{ marginTop: "auto" }}>
        <button className="rail-item rail-add" title="Configurações" onClick={onSettings}>
          <SidebarIcon name="settings" />
        </button>
      </div>
    </nav>
  );
}

// ---------------------------------------------------------------------------
// Lista de canais
// ---------------------------------------------------------------------------

function ChannelList({
  detail, conn, activeChannelId, voiceChannelId, souDono,
  onPick, onEntrarNaVoz, onCriarCanal, onConvidar,
}) {
  if (!detail) return <aside className="canais"><div className="empty">…</div></aside>;

  const texto = detail.channels.filter((c) => c.kind === "text");
  const voz   = detail.channels.filter((c) => c.kind === "voice");

  const rotuloConn =
    conn === "open" ? "conectado" :
    conn === "reconnecting" ? "reconectando" :
    conn === "connecting" ? "conectando" : "desconectado";

  return (
    <aside className="canais">
      <header className="canais-head">
        <div>
          <div className="srv-nome">{detail.name}</div>
          <div className="srv-meta mono">
            {detail.member_count} {detail.member_count === 1 ? "membro" : "membros"}
            <span className={`conn conn-${conn === "open" ? "ok" : conn === "reconnecting" ? "wait" : "off"}`}>
              ● {rotuloConn}
            </span>
          </div>
        </div>
        <button className="btn-ghost" title="Convidar" onClick={onConvidar}>convidar</button>
      </header>

      <div className="grupo">
        <div className="grupo-head">
          <span>canais de texto</span>
          {souDono && <button className="mini" title="Novo canal" onClick={onCriarCanal}>+</button>}
        </div>
        {texto.map((c) => (
          <button
            key={c.id}
            className={`canal ${c.id === activeChannelId ? "on" : ""} ${c.unread > 0 ? "tem-novo" : ""}`}
            onClick={() => onPick(c.id)}
          >
            <span className="hash">#</span>
            <span className="nome">{c.name}</span>
            {c.unread > 0 && <span className="badge">{c.unread > 99 ? "99+" : c.unread}</span>}
          </button>
        ))}
        {!texto.length && <div className="empty sm">nenhum canal de texto</div>}
      </div>

      <div className="grupo">
        <div className="grupo-head"><span>canais de voz</span></div>
        {voz.map((c) => (
          <div key={c.id} className="canal-voz-wrap">
            <button
              className={`canal ${c.id === activeChannelId ? "on" : ""}`}
              onClick={() => onPick(c.id)}
              onDoubleClick={() => onEntrarNaVoz(c)}
              title="Duplo clique entra na call"
            >
              <span className="hash">🔊</span>
              <span className="nome">{c.name}</span>
              {c.live > 0 && <span className="live-dot mono">{c.live}</span>}
              {c.id === voiceChannelId && <span className="voce mono">você</span>}
            </button>
          </div>
        ))}
        {!voz.length && <div className="empty sm">nenhum canal de voz</div>}
      </div>
    </aside>
  );
}

// ---------------------------------------------------------------------------
// Canal de texto
// ---------------------------------------------------------------------------

function TextChannel({ canal, rt, session, onLido, onMensagemDeOutroCanal }) {
  const [mensagens, setMensagens] = useS2([]);
  const [cursor, setCursor] = useS2(null);
  const [carregando, setCarregando] = useS2(true);
  const [carregandoMais, setCarregandoMais] = useS2(false);
  const [texto, setTexto] = useS2("");
  const [erro, setErro] = useS2("");
  const listaRef = useR2(null);
  const noFimRef = useR2(true);

  // --- histórico inicial ---------------------------------------------------
  useE2(() => {
    let vivo = true;
    setCarregando(true);
    window.api.listMessages(canal.slug, { limit: 50 })
      .then((p) => {
        if (!vivo) return;
        // A API devolve mais novas primeiro; a tela renderiza de cima pra baixo.
        setMensagens([...p.messages].reverse());
        setCursor(p.next_before);
        setCarregando(false);
      })
      .catch((e) => { if (vivo) { setErro(e.message || "não deu pra carregar"); setCarregando(false); } });
    return () => { vivo = false; };
  }, [canal.slug]);

  // --- tempo real ----------------------------------------------------------
  useE2(() => {
    if (!rt) return;
    const offs = [
      rt.on("message", (m) => {
        if (m.channel_id !== canal.id) { onMensagemDeOutroCanal(m.channel_id); return; }
        setMensagens((prev) => (prev.some((x) => x.id === m.message.id) ? prev : [...prev, m.message]));
      }),
      rt.on("message_updated", (m) => {
        if (m.channel_id !== canal.id) return;
        setMensagens((prev) => prev.map((x) => (x.id === m.message.id ? m.message : x)));
      }),
      rt.on("message_deleted", (m) => {
        if (m.channel_id !== canal.id) return;
        setMensagens((prev) => prev.filter((x) => x.id !== m.message_id));
      }),
    ];
    return () => offs.forEach((off) => off());
  }, [rt, canal.id, onMensagemDeOutroCanal]);

  // --- rolagem -------------------------------------------------------------
  // Só cola no fim se o usuário já estava no fim. Puxar histórico e ser jogado
  // pra baixo por uma mensagem nova é o jeito mais rápido de irritar alguém.
  useE2(() => {
    const el = listaRef.current;
    if (el && noFimRef.current) el.scrollTop = el.scrollHeight;
  }, [mensagens]);

  const aoRolar = () => {
    const el = listaRef.current;
    if (!el) return;
    noFimRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    if (el.scrollTop < 60 && cursor && !carregandoMais) carregarMais();
  };

  const carregarMais = async () => {
    const el = listaRef.current;
    if (!el || !cursor) return;
    setCarregandoMais(true);
    const alturaAntes = el.scrollHeight;
    try {
      const p = await window.api.listMessages(canal.slug, { before: cursor, limit: 50 });
      setMensagens((prev) => [...[...p.messages].reverse(), ...prev]);
      setCursor(p.next_before);
      // Mantém o ponto de leitura: sem isso a lista salta ao inserir por cima.
      requestAnimationFrame(() => { el.scrollTop = el.scrollHeight - alturaAntes; });
    } catch (e) {
      setErro(e.message || "não deu pra carregar mais");
    } finally {
      setCarregandoMais(false);
    }
  };

  // --- marcar lido ---------------------------------------------------------
  useE2(() => {
    if (carregando || !mensagens.length) return;
    const ultima = mensagens[mensagens.length - 1];
    window.api.markRead(canal.slug, ultima.id).then(() => onLido(canal.id)).catch(() => {});
  }, [carregando, mensagens.length, canal.slug, canal.id, onLido]);

  const enviar = async () => {
    const corpo = texto.trim();
    if (!corpo) return;
    setTexto("");
    noFimRef.current = true;
    try {
      const m = await window.api.sendMessage(canal.slug, corpo);
      setMensagens((prev) => (prev.some((x) => x.id === m.id) ? prev : [...prev, m]));
    } catch (e) {
      setErro(e.message || "não deu pra enviar");
      setTexto(corpo); // devolve o texto pro campo em vez de sumir com ele
    }
  };

  return (
    <div className="chat">
      <header className="chat-head">
        <span className="hash">#</span>
        <b>{canal.name}</b>
        <CopyButton
          text={`${publicUrl()}/c/${canal.slug}`}
          label="link do canal"
          done="link copiado!"
          style={{ marginLeft: "auto" }}
        />
      </header>

      <div className="msgs" ref={listaRef} onScroll={aoRolar}>
        {carregandoMais && <div className="empty sm">carregando mais…</div>}
        {!carregando && !cursor && (
          <div className="inicio mono">— começo de #{canal.name} —</div>
        )}
        {carregando && <div className="empty">carregando…</div>}
        {!carregando && !mensagens.length && (
          <div className="empty">Ninguém falou nada aqui ainda. Começa você.</div>
        )}
        {mensagens.map((m, i) => {
          const anterior = mensagens[i - 1];
          const agrupada = anterior && anterior.user_id === m.user_id &&
            new Date(m.created_at) - new Date(anterior.created_at) < 5 * 60 * 1000;
          return (
            <Mensagem
              key={m.id}
              m={m}
              agrupada={agrupada}
              minha={m.user_id === session.id}
              onApagar={async () => {
                try {
                  await window.api.deleteMessage(m.id);
                  setMensagens((prev) => prev.filter((x) => x.id !== m.id));
                } catch (e) { setErro(e.message || "não deu pra apagar"); }
              }}
            />
          );
        })}
      </div>

      {erro && <div className="form-error mono" style={{ margin: "0 16px" }}>{erro}</div>}

      <div className="composer">
        <input
          className="input"
          placeholder={`escreve em #${canal.name}…`}
          value={texto}
          maxLength={4000}
          onChange={(e) => { setTexto(e.target.value); setErro(""); }}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); enviar(); } }}
        />
        <button className="btn-primary" disabled={!texto.trim()} onClick={enviar}>enviar</button>
      </div>
    </div>
  );
}

function Mensagem({ m, agrupada, minha, onApagar }) {
  const hora = new Date(m.created_at).toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });
  return (
    <div className={`msg ${agrupada ? "agrupada" : ""}`}>
      {!agrupada && <span className="initials sm">{iniciais(m.display_name)}</span>}
      <div className="msg-corpo">
        {!agrupada && (
          <div className="msg-head">
            <b>{m.display_name || "alguém"}</b>
            <span className="mono dim">{hora}</span>
            {m.edited_at && <span className="mono dim">(editada)</span>}
          </div>
        )}
        <div className="msg-texto">{m.body}</div>
      </div>
      {minha && <button className="msg-x" title="Apagar" onClick={onApagar}>×</button>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Canal de voz
// ---------------------------------------------------------------------------

function VoiceChannelPanel({ canal, emCall, peers, onEntrar, onSair, erro }) {
  return (
    <div className="voz-painel">
      <header className="chat-head">
        <span className="hash">🔊</span>
        <b>{canal.name}</b>
        <CopyButton
          text={`${publicUrl()}/c/${canal.slug}`}
          label="link do canal"
          done="link copiado!"
          style={{ marginLeft: "auto" }}
        />
      </header>
      <div className="voz-centro">
        <div className="voz-count mono">
          {canal.live || 0} {canal.live === 1 ? "pessoa" : "pessoas"} no canal
        </div>
        {emCall ? (
          <>
            <div className="list" style={{ width: "100%", maxWidth: 420 }}>
              {peers.map((p) => (
                <UserRow key={p.id} user={{ ...p, state: p.muted ? "muted" : (p.level > 0.05 ? "speaking" : "listening") }} />
              ))}
              {!peers.length && <div className="empty">Só você por aqui. Compartilha o link do canal.</div>}
            </div>
            <button className="fbtn fbtn-leave" onClick={onSair}>Sair da call</button>
          </>
        ) : (
          <button className="btn-primary" onClick={onEntrar}>Entrar na call</button>
        )}
        {erro && <div className="form-error mono">{erro}</div>}
      </div>
    </div>
  );
}

/// Barra fixa de call — fica visível mesmo navegando pra outro canal.
function VoiceBar({ canal, peers, muted, setMuted, onSair, session, erro }) {
  const falando = peers.filter((p) => !p.muted && p.level > 0.05).length;
  return (
    <div className="voz-bar">
      <span className="dotmic" />
      <div className="vb-info">
        <b>{canal?.name || "call"}</b>
        <span className="mono dim">
          {peers.length + 1} na call{falando ? ` · ${falando} falando` : ""}
        </span>
      </div>
      {erro && <span className="form-error mono" style={{ padding: "2px 8px" }}>{erro}</span>}
      <button className={`fbtn ${muted ? "fbtn-mute-on" : ""}`} onClick={() => setMuted(!muted)}>
        {muted ? "Ativar mic" : "Silenciar"} <kbd>M</kbd>
      </button>
      <button className="fbtn fbtn-leave" onClick={onSair}>Sair</button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Primeiro acesso — ninguém tem servidor ainda
// ---------------------------------------------------------------------------

function PrimeiroServidor({ onCriar, onEntrar, erro }) {
  const [nome, setNome] = useS2("");
  const [link, setLink] = useS2("");
  const [busy, setBusy] = useS2(false);
  const [err, setErr] = useS2("");

  const tentar = async (fn, arg) => {
    setBusy(true); setErr("");
    try { await fn(arg); } catch (e) { setErr(e.message || "não deu certo"); }
    finally { setBusy(false); }
  };

  return (
    <div className="auth" style={{ maxWidth: 460, margin: "auto" }}>
      <div className="auth-head">
        <h2>Cria teu primeiro servidor</h2>
        <div className="sub">Um servidor guarda teus canais de texto e de voz.</div>
      </div>
      <div className="auth-body">
        <label className="label">nome do servidor</label>
        <input className="input" placeholder="Time Athmos" value={nome}
          onChange={(e) => setNome(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && nome.trim().length >= 2) tentar(onCriar, nome.trim()); }} />
        <button className="btn-primary" disabled={nome.trim().length < 2 || busy}
          onClick={() => tentar(onCriar, nome.trim())}>
          {busy ? "criando…" : "criar servidor"}
        </button>

        <div className="divider" style={{ margin: "14px 0" }} />

        <label className="label">ou entra num que te convidaram</label>
        <input className="input mono" placeholder="cola o link do canal" value={link}
          onChange={(e) => setLink(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && link.trim()) tentar(onEntrar, link.trim()); }} />
        <button className="btn-line" disabled={!link.trim() || busy}
          onClick={() => tentar(onEntrar, link.trim())}>entrar</button>

        {(err || erro) && <div className="form-error mono">{err || erro}</div>}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Modais
// ---------------------------------------------------------------------------

function ModalSimples({ titulo, rotulo, placeholder, cta, mono, onFechar, onEnviar }) {
  const [v, setV] = useS2("");
  const [busy, setBusy] = useS2(false);
  const [err, setErr] = useS2("");

  const enviar = async () => {
    if (!v.trim() || busy) return;
    setBusy(true); setErr("");
    try { await onEnviar(v.trim()); }
    catch (e) { setErr(e.message || "não deu certo"); setBusy(false); }
  };

  return (
    <div className="modal-overlay" onClick={onFechar}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>{titulo}</h3>
          <button className="x" onClick={onFechar}>×</button>
        </div>
        <div className="modal-body">
          <div className="label">{rotulo}</div>
          <input className={`input ${mono ? "mono" : ""}`} placeholder={placeholder} value={v} autoFocus
            onChange={(e) => { setV(e.target.value); setErr(""); }}
            onKeyDown={(e) => { if (e.key === "Enter") enviar(); }} />
          {err && <div className="form-error mono">{err}</div>}
        </div>
        <div className="modal-foot">
          <button className="btn-ghost" onClick={onFechar}>cancelar</button>
          <button className="btn-primary" disabled={!v.trim() || busy} onClick={enviar}>
            {busy ? "…" : cta}
          </button>
        </div>
      </div>
    </div>
  );
}

function CriarCanal({ onFechar, onEnviar }) {
  const [nome, setNome] = useS2("");
  const [tipo, setTipo] = useS2("text");
  const [busy, setBusy] = useS2(false);
  const [err, setErr] = useS2("");

  const enviar = async () => {
    if (!nome.trim() || busy) return;
    setBusy(true); setErr("");
    try { await onEnviar({ nome: nome.trim(), tipo }); }
    catch (e) { setErr(e.message || "não deu certo"); setBusy(false); }
  };

  return (
    <div className="modal-overlay" onClick={onFechar}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>Novo canal</h3>
          <button className="x" onClick={onFechar}>×</button>
        </div>
        <div className="modal-body">
          <div className="label">tipo</div>
          <div className="seg" style={{ marginTop: 6 }}>
            <button className={`seg-btn ${tipo === "text" ? "seg-on" : ""}`} onClick={() => setTipo("text")}># texto</button>
            <button className={`seg-btn ${tipo === "voice" ? "seg-on" : ""}`} onClick={() => setTipo("voice")}>🔊 voz</button>
          </div>
          <div className="label" style={{ marginTop: 12 }}>nome</div>
          <input className="input" placeholder={tipo === "text" ? "deploys" : "Pair"} value={nome} autoFocus
            maxLength={32}
            onChange={(e) => { setNome(e.target.value); setErr(""); }}
            onKeyDown={(e) => { if (e.key === "Enter") enviar(); }} />
          {err && <div className="form-error mono">{err}</div>}
        </div>
        <div className="modal-foot">
          <button className="btn-ghost" onClick={onFechar}>cancelar</button>
          <button className="btn-primary" disabled={!nome.trim() || busy} onClick={enviar}>criar</button>
        </div>
      </div>
    </div>
  );
}

function Convite({ detail, onFechar }) {
  const voz = (detail?.channels || []).filter((c) => c.kind === "voice");
  const texto = (detail?.channels || []).filter((c) => c.kind === "text");
  const base = publicUrl();
  return (
    <div className="modal-overlay" onClick={onFechar}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3>Convidar pro {detail?.name}</h3>
          <button className="x" onClick={onFechar}>×</button>
        </div>
        <div className="modal-body">
          <div className="dim" style={{ fontSize: 12 }}>
            Quem abrir o link entra no servidor e cai direto no canal.
          </div>
          {[...voz, ...texto].map((c) => (
            <div key={c.id} className="link-row">
              <span className="hash">{c.kind === "voice" ? "🔊" : "#"}</span>
              <span className="nome">{c.name}</span>
              <CopyButton text={`${base}/c/${c.slug}`} label="copiar" done="copiado!" />
            </div>
          ))}
        </div>
        <div className="modal-foot">
          <button className="btn-ghost" onClick={onFechar}>fechar</button>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Configurações
// ---------------------------------------------------------------------------

function Settings({ go, session, setSession, tweaks, setTweak }) {
  const [tab, setTab] = useS2("perfil");
  const [nome, setNome] = useS2(session.displayName || "");
  const [salvando, setSalvando] = useS2(false);
  const [msg, setMsg] = useS2("");
  const [err, setErr] = useS2("");

  // Salvar precisa ir pro servidor: mexer só no estado local faz o nome voltar
  // sozinho no próximo boot, porque o boot lê de api.me().
  const salvar = async () => {
    setSalvando(true); setErr(""); setMsg("");
    try {
      const u = await window.api.updateMe({ displayName: nome.trim() });
      setSession((s) => ({ ...s, displayName: u.display_name }));
      setMsg("salvo");
    } catch (e) {
      setErr(e.message || "não deu pra salvar");
    } finally {
      setSalvando(false);
    }
  };

  return (
    <div className="settings">
      <div className="appbar">
        <div className="brand-row">
          <Mark size={14} />
          <span className="bn"><b>bora</b>call</span>
          <span className="room-crumb"><span className="slash">/</span>configurações</span>
        </div>
        <button className="btn-ghost" onClick={() => go("app")}>← voltar</button>
      </div>
      <div className="set-tabs">
        {[["perfil", "perfil"], ["preferencias", "preferências"], ["conta", "conta"]].map(([k, l]) => (
          <button key={k} className={`set-tab ${tab === k ? "on" : ""}`} onClick={() => setTab(k)}>{l}</button>
        ))}
      </div>
      <div className="set-body">
        {tab === "perfil" && (
          <>
            <div className="set-row">
              <div className="k">Nome <span className="d">aparece pros outros nos canais</span></div>
              <input className="input" value={nome} maxLength={64}
                onChange={(e) => { setNome(e.target.value); setMsg(""); }} />
              <span />
            </div>
            <div className="set-row">
              <div className="k">E-mail <span className="d">usado pra login</span></div>
              <div className="mono dim">{session.email}</div>
              <span />
            </div>
            {err && <div className="form-error mono">{err}</div>}
            <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 8, alignItems: "center" }}>
              {msg && <span className="mono dim">{msg}</span>}
              <button className="btn-primary" disabled={nome.trim().length < 1 || salvando} onClick={salvar}>
                {salvando ? "salvando…" : "salvar"}
              </button>
            </div>
          </>
        )}
        {tab === "preferencias" && <TweaksPanel tweaks={tweaks} setTweak={setTweak} />}
        {tab === "conta" && (
          <>
            <div className="set-row">
              <div className="k">Limite por canal de voz
                <span className="d">a chamada é P2P — acima disso o áudio degrada</span></div>
              <div className="mono">até 6 pessoas</div>
              <span />
            </div>
            <div className="divider" style={{ margin: "10px 0" }} />
            <div className="set-row">
              <div className="k" style={{ color: "var(--fg-dim)" }}>Sair da conta</div>
              <span />
              <button className="btn-line" onClick={() => {
                try { window.api.logout(); } catch {}
                try { localStorage.removeItem("bc_server"); } catch {}
                setSession({ email: "", displayName: "", id: null });
                go("landing");
              }}>sair</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

Object.assign(window, {
  AppShell, ServerRail, ChannelList, TextChannel, Mensagem,
  VoiceChannelPanel, VoiceBar, PrimeiroServidor, ModalSimples, CriarCanal, Convite, Settings,
});

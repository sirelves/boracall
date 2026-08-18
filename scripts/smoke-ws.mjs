// Smoke end-to-end do WebSocket por servidor.
//
// Sobe dois usuários de verdade, cria servidor, conecta dois sockets, entra no
// canal de voz e troca signaling. Sem mock e sem dependência: usa o WebSocket
// nativo do Node (>= 22) contra o binário rodando.
//
// Os testes de integração cobrem as regras do Hub em memória; este script cobre
// o que eles não alcançam — o socket de verdade, o handshake, o roteamento e a
// ponte entre a escrita HTTP e o evento em tempo real.
//
//   cargo run -p boracall-server        # num terminal
//   node scripts/smoke-ws.mjs           # noutro
//
// API=https://api.boracall.com node scripts/smoke-ws.mjs  → roda contra outro host.

const API = process.env.API || "http://127.0.0.1:3030";
const WS = API.replace(/^http/, "ws");

let falhas = 0;
const ok = (cond, msg) => {
  console.log(`${cond ? "  ok  " : " FALHA"} ${msg}`);
  if (!cond) falhas++;
};

async function api(path, { method = "GET", token, body } = {}) {
  const res = await fetch(API + path, {
    method,
    headers: {
      "content-type": "application/json",
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
    ...(body ? { body: JSON.stringify(body) } : {}),
  });
  const text = await res.text();
  let json = null;
  try { json = JSON.parse(text); } catch {}
  if (!res.ok) throw new Error(`${method} ${path} → ${res.status} ${text.slice(0, 200)}`);
  return json;
}

function connect(slug, token, label) {
  const sock = new WebSocket(`${WS}/ws/servers/${slug}`, ["bc.v1", `token.${token}`]);
  const recebidas = [];
  const esperando = [];
  sock.addEventListener("message", (ev) => {
    const m = JSON.parse(ev.data);
    recebidas.push(m);
    for (let i = esperando.length - 1; i >= 0; i--) {
      if (esperando[i].pred(m)) {
        esperando[i].resolve(m);
        esperando.splice(i, 1);
      }
    }
  });
  return {
    label,
    sock,
    recebidas,
    aberto: new Promise((res, rej) => {
      sock.addEventListener("open", res);
      sock.addEventListener("error", () => rej(new Error(`${label}: erro no socket`)));
      sock.addEventListener("close", (e) =>
        rej(new Error(`${label}: fechou (code ${e.code})`)));
    }),
    send: (o) => sock.send(JSON.stringify(o)),
    espera(pred, oque, ms = 3000) {
      const achada = this.recebidas.find(pred);
      if (achada) return Promise.resolve(achada);
      return new Promise((resolve, reject) => {
        const t = setTimeout(
          () => reject(new Error(`${label}: timeout esperando ${oque}`)), ms);
        esperando.push({ pred: (m) => { if (pred(m)) { clearTimeout(t); return true; } return false; }, resolve });
      });
    },
  };
}

const rnd = () => Math.random().toString(36).slice(2, 10);

async function main() {
  console.log("→ health");
  ok((await api("/api/health").catch(() => null)) !== undefined, "server no ar");

  console.log("\n→ criando dois usuários");
  const a = await api("/api/auth/signup", {
    method: "POST",
    body: { email: `smoke-${rnd()}@teste.local`, password: "senha-de-teste-123", display_name: "Ana" },
  });
  const b = await api("/api/auth/signup", {
    method: "POST",
    body: { email: `smoke-${rnd()}@teste.local`, password: "senha-de-teste-123", display_name: "Bruno" },
  });
  ok(!!a.token && !!b.token, "dois tokens emitidos");

  console.log("\n→ Ana cria servidor");
  const srv = await api("/api/servers", {
    method: "POST", token: a.token, body: { name: "Smoke Test" },
  });
  const canalVoz = srv.channels.find((c) => c.kind === "voice");
  const canalTexto = srv.channels.find((c) => c.kind === "text");
  ok(!!canalVoz && !!canalTexto, "servidor nasceu com canal de voz e de texto");

  console.log("\n→ Bruno entra pelo link");
  await api(`/api/servers/${srv.slug}/join`, { method: "POST", token: b.token });

  console.log("\n→ conectando os dois websockets");
  const wa = connect(srv.slug, a.token, "Ana");
  const wb = connect(srv.slug, b.token, "Bruno");
  await Promise.all([wa.aberto, wb.aberto]);
  ok(true, "handshake aceito com JWT no subprotocol");

  await wa.espera((m) => m.type === "voice_state", "voice_state inicial");
  ok(true, "snapshot de voz chega ao conectar");

  console.log("\n→ Ana entra no canal de voz");
  wa.send({ type: "join_voice", channel_id: canalVoz.id });
  const presencaB = await wb.espera(
    (m) => m.type === "voice_joined" && m.channel_id === canalVoz.id, "voice_joined em Bruno");
  ok(presencaB.peer.display_name === "Ana", "Bruno vê Ana entrando no canal de voz");

  console.log("\n→ Bruno entra no mesmo canal");
  wb.send({ type: "join_voice", channel_id: canalVoz.id });
  const pres = await wa.espera(
    (m) => m.type === "voice_presence" && m.channel_id === canalVoz.id && m.peers.length === 2,
    "presença com 2 pessoas");
  ok(pres.peers.length === 2, "canal de voz com os dois");

  console.log("\n→ signaling WebRTC entre os dois");
  wa.send({ type: "offer", to: presencaB.peer.user_id === a.user?.id ? b.user.id : b.user.id, sdp: "v=0 fake-sdp" });
  const offer = await wb.espera((m) => m.type === "offer", "offer em Bruno");
  ok(offer.sdp === "v=0 fake-sdp", "offer chega no par certo");

  console.log("\n→ mute propaga");
  wa.send({ type: "mute", muted: true });
  const mute = await wb.espera((m) => m.type === "mute" && m.muted === true, "mute");
  ok(mute.channel_id === canalVoz.id, "mute vem carimbado com o canal");

  console.log("\n→ mensagem de texto via HTTP aparece no WebSocket");
  const msg = await api(`/api/channels/${canalTexto.slug}/messages`, {
    method: "POST", token: b.token, body: { body: "chegou em tempo real?" },
  });
  const viaWs = await wa.espera((m) => m.type === "message", "message em Ana");
  ok(viaWs.message.id === msg.id, "a escrita HTTP virou evento no socket");
  ok(viaWs.message.body === "chegou em tempo real?", "corpo da mensagem intacto");

  console.log("\n→ quem enviou não recebe eco da própria mensagem");
  const ecoEmB = wb.recebidas.filter((m) => m.type === "message" && m.message?.id === msg.id);
  ok(ecoEmB.length === 0, "sem eco pro autor");

  console.log("\n→ live count aparece no detalhe do servidor");
  const detalhe = await api(`/api/servers/${srv.slug}`, { token: a.token });
  const vozDetalhe = detalhe.channels.find((c) => c.id === canalVoz.id);
  ok(vozDetalhe.live === 2, `canal de voz reporta 2 ao vivo (veio ${vozDetalhe.live})`);

  console.log("\n→ Ana sai do canal de voz");
  wa.send({ type: "leave_voice" });
  await wb.espera((m) => m.type === "voice_left" && m.user_id, "voice_left em Bruno");
  const depois = await api(`/api/servers/${srv.slug}`, { token: a.token });
  ok(depois.channels.find((c) => c.id === canalVoz.id).live === 1, "live desce pra 1");

  console.log("\n→ offer pra quem não está no canal é recusado");
  wa.send({ type: "offer", to: b.user.id, sdp: "fora-do-canal" });
  const erro = await wa.espera((m) => m.type === "error", "erro de canal");
  ok(/canal de voz/.test(erro.message), `servidor recusa signaling fora do canal: "${erro.message}"`);

  console.log("\n→ desconexão limpa a presença");
  wb.sock.close();
  await new Promise((r) => setTimeout(r, 400));
  const final = await api(`/api/servers/${srv.slug}`, { token: a.token });
  ok(final.channels.find((c) => c.id === canalVoz.id).live === 0, "socket caiu, presença zerou");

  wa.sock.close();
  console.log(falhas === 0 ? "\nTUDO VERDE" : `\n${falhas} FALHA(S)`);
  process.exit(falhas === 0 ? 0 : 1);
}

main().catch((e) => { console.error("\nERRO:", e.message); process.exit(1); });

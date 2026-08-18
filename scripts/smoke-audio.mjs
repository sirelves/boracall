// Smoke de áudio ponta a ponta: dois navegadores num canal de voz, com
// microfone sintético, e conferência de que RTP de áudio realmente trafega.
//
// O smoke-ws.mjs cobre o signaling (quem fala com quem). Este aqui cobre o que
// vem depois: a mesh WebRTC negocia, o ICE conecta e os pacotes de áudio
// chegam do outro lado. É o mais perto de "duas pessoas se ouvindo" que dá pra
// automatizar — o que ele não prova é a captação do microfone físico e a saída
// no alto-falante.
//
// Pré-requisitos:
//   cargo run -p boracall-server          # backend em 127.0.0.1:3030
//   (cd dist && python3 -m http.server 5174)
//   npm i playwright && npx playwright install chromium
//
// Uso:
//   node scripts/smoke-audio.mjs
//   API=... APP=... node scripts/smoke-audio.mjs

import { chromium } from "playwright";

const API = process.env.API || "http://127.0.0.1:3030";
const APP = process.env.APP || "http://127.0.0.1:5174/index.html";
// Relay demora mais pra conectar que o caminho direto: cada lado precisa
// alocar no TURN, criar permissão e fazer channel bind antes de qualquer mídia.
const ESPERA_MS = Number(process.env.ESPERA_MS || (process.env.RELAY_ONLY === "1" ? 20000 : 8000));
// RELAY_ONLY=1 força todo o tráfego pelo TURN (iceTransportPolicy: "relay").
// É o único jeito de provar que o relay funciona sem estar atrás de NAT
// simétrica de verdade: se o áudio passa assim, passou pelo coturn.
const RELAY_ONLY = process.env.RELAY_ONLY === "1";

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
  const txt = await res.text();
  if (!res.ok) throw new Error(`${method} ${path} → ${res.status} ${txt.slice(0, 200)}`);
  return txt ? JSON.parse(txt) : null;
}

const rnd = () => Math.random().toString(36).slice(2, 10);

/// Sobe a stack de voz na página usando os globais reais do app.
async function entrarNaCall(page, { token, user, serverSlug, channelId }) {
  await page.addInitScript(([t, u]) => {
    localStorage.setItem("bc_token", t);
    localStorage.setItem("bc_user", u);
  }, [token, JSON.stringify(user)]);

  await page.goto(APP);
  await page.waitForFunction(() => window.Realtime && window.WebRTCMesh && window.api);

  return page.evaluate(async ([slug, chId, selfId, relayOnly]) => {
    const rt = new window.Realtime(slug);
    const mesh = new window.WebRTCMesh.Mesh(rt, selfId, {
      channelId: chId,
      ...(relayOnly ? { iceTransportPolicy: "relay" } : {}),
    });
    // Guardados pra que a fase de medição consiga alcançá-los.
    window.__rt = rt;
    window.__mesh = mesh;

    const aberto = new Promise((res) => rt.on("_state", (s) => s === "open" && res()));
    rt.connect();
    await aberto;

    await mesh.start();          // acquireMic() pega o dispositivo falso do Chrome
    rt.joinVoice(chId);
    return { pares: mesh.peers.size };
  }, [serverSlug, channelId, user.id, RELAY_ONLY]);
}

/// Lê as estatísticas de RTP de cada peer connection da mesh.
async function medir(page) {
  return page.evaluate(async () => {
    const out = [];
    for (const [userId, p] of window.__mesh.peers) {
      const stats = await p.pc.getStats(null);
      let entrada = null, saida = null, parIce = null;
      stats.forEach((r) => {
        if (r.type === "inbound-rtp" && r.kind === "audio") {
          entrada = { bytes: r.bytesReceived || 0, pacotes: r.packetsReceived || 0 };
        }
        if (r.type === "outbound-rtp" && r.kind === "audio") {
          saida = { bytes: r.bytesSent || 0, pacotes: r.packetsSent || 0 };
        }
        if (r.type === "candidate-pair" && r.state === "succeeded") {
          parIce = { rtt: r.currentRoundTripTime ?? null, localId: r.localCandidateId };
        }
      });
      // Tipo do candidato local vencedor: "relay" prova que passou pelo TURN.
      let tipoLocal = null;
      if (parIce?.localId) {
        const c = stats.get(parIce.localId);
        if (c) tipoLocal = c.candidateType;
      }

      out.push({
        userId,
        tipoLocal,
        connectionState: p.pc.connectionState,
        iceConnectionState: p.pc.iceConnectionState,
        temStreamRemota: !!p.stream,
        entrada, saida, parIce,
      });
    }
    return out;
  });
}

async function main() {
  console.log("→ preparando servidor e dois usuários");
  const a = await api("/api/auth/signup", {
    method: "POST",
    body: { email: `audio-a-${rnd()}@teste.local`, password: "senha-de-teste-123", display_name: "Ana" },
  });
  const b = await api("/api/auth/signup", {
    method: "POST",
    body: { email: `audio-b-${rnd()}@teste.local`, password: "senha-de-teste-123", display_name: "Bruno" },
  });
  const srv = await api("/api/servers", { method: "POST", token: a.token, body: { name: "Audio Test" } });
  await api(`/api/servers/${srv.slug}/join`, { method: "POST", token: b.token });
  const canalVoz = srv.channels.find((c) => c.kind === "voice");
  ok(!!canalVoz, "servidor com canal de voz criado");

  console.log(`→ abrindo dois navegadores com microfone sintético${RELAY_ONLY ? " (ICE só por relay)" : ""}`);
  const browser = await chromium.launch({
    args: [
      "--use-fake-device-for-media-stream",   // microfone sintético (tom contínuo)
      "--use-fake-ui-for-media-stream",       // sem diálogo de permissão
      "--autoplay-policy=no-user-gesture-required",
    ],
  });

  try {
    const ctxA = await browser.newContext({ permissions: ["microphone"] });
    const ctxB = await browser.newContext({ permissions: ["microphone"] });
    const pageA = await ctxA.newPage();
    const pageB = await ctxB.newPage();

    const erros = [];
    for (const [nome, pg] of [["Ana", pageA], ["Bruno", pageB]]) {
      pg.on("pageerror", (e) => erros.push(`${nome}: ${e.message}`));
    }

    await entrarNaCall(pageA, { token: a.token, user: a.user, serverSlug: srv.slug, channelId: canalVoz.id });
    await entrarNaCall(pageB, { token: b.token, user: b.user, serverSlug: srv.slug, channelId: canalVoz.id });
    ok(erros.length === 0, `sem erro de página${erros.length ? ": " + erros.join(" | ") : ""}`);

    console.log(`→ deixando o áudio correr por ${ESPERA_MS / 1000}s`);
    await new Promise((r) => setTimeout(r, ESPERA_MS));

    const statsA = await medir(pageA);
    const statsB = await medir(pageB);

    ok(statsA.length === 1, `Ana enxerga 1 par (veio ${statsA.length})`);
    ok(statsB.length === 1, `Bruno enxerga 1 par (veio ${statsB.length})`);

    for (const [nome, s] of [["Ana", statsA[0]], ["Bruno", statsB[0]]]) {
      if (!s) { ok(false, `${nome}: nenhuma peer connection`); continue; }
      ok(s.connectionState === "connected", `${nome}: conexão WebRTC estabelecida (${s.connectionState})`);
      ok(s.temStreamRemota, `${nome}: recebeu a stream remota`);
      ok((s.saida?.pacotes || 0) > 0, `${nome}: enviou áudio (${s.saida?.pacotes || 0} pacotes, ${s.saida?.bytes || 0} bytes)`);
      ok((s.entrada?.pacotes || 0) > 0, `${nome}: RECEBEU áudio (${s.entrada?.pacotes || 0} pacotes, ${s.entrada?.bytes || 0} bytes)`);
      console.log(`       candidato local: ${s.tipoLocal ?? "?"}${s.parIce?.rtt != null ? ` · rtt ${Math.round(s.parIce.rtt * 1000)}ms` : ""}`);
      if (RELAY_ONLY) {
        ok(s.tipoLocal === "relay", `${nome}: caminho é relay (TURN), não direto`);
      }
    }
  } finally {
    await browser.close();
  }

  console.log(falhas === 0 ? "\nÁUDIO TRAFEGA NOS DOIS SENTIDOS" : `\n${falhas} FALHA(S)`);
  process.exit(falhas === 0 ? 0 : 1);
}

main().catch((e) => { console.error("\nERRO:", e.message); process.exit(1); });

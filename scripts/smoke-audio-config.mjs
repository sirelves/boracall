// Smoke da escolha de dispositivo de áudio.
//
// Cobre o que a issue #47 pediu — listar e trocar microfone e alto-falante — e
// um problema achado ao implementar: abrir Configurações DERRUBAVA a chamada,
// porque a tela era uma rota e desmontava o app. Era justo o momento em que
// alguém iria trocar de dispositivo.
//
// Pré-requisitos: backend em 127.0.0.1:3030, dist/ servido em 5174, playwright
// com chromium instalado. Uso: node scripts/smoke-audio-config.mjs
import { chromium } from "playwright";
const API = "http://127.0.0.1:3030", APP = "http://127.0.0.1:5174/index.html";
let falhas = 0;
const ok = (c, m) => { console.log(`${c ? "  ok  " : " FALHA"} ${m}`); if (!c) falhas++; };
const j = async (p, o = {}) => {
  const r = await fetch(API + p, { method: o.method || "GET",
    headers: { "content-type": "application/json", ...(o.token ? { authorization: "Bearer " + o.token } : {}) },
    body: o.body ? JSON.stringify(o.body) : undefined });
  const t = await r.text(); if (!r.ok) throw new Error(`${p} ${r.status} ${t}`);
  return t ? JSON.parse(t) : null;
};
const rnd = () => Math.random().toString(36).slice(2, 10);

const u = await j("/api/auth/signup", { method: "POST", body: { email: `aud-${rnd()}@teste.local`, password: "senha-de-teste-123", display_name: "Ana" } });
await j("/api/servers", { method: "POST", token: u.token, body: { name: "Servidor de Teste" } });

const browser = await chromium.launch({ args: ["--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream"] });
const ctx = await browser.newContext({ permissions: ["microphone"] });
const pg = await ctx.newPage();
const erros = []; pg.on("pageerror", e => erros.push(e.message));

await pg.addInitScript(([t, us]) => {
  localStorage.setItem("bc_token", t); localStorage.setItem("bc_user", us);
  localStorage.setItem("bc_route", JSON.stringify("app"));
}, [u.token, JSON.stringify(u.user)]);
await pg.goto(APP);
await pg.waitForFunction(() => [...document.querySelectorAll(".canal .hash")].some(e => e.textContent === "🔊"), { timeout: 15000 });
ok(erros.length === 0, `sem erro de página${erros.length ? ": " + erros[0] : ""}`);

console.log("\n→ a aba de áudio lista dispositivos reais");
await pg.click(".rail-item[title='Configurações']");
await pg.waitForSelector(".set-tabs", { timeout: 10000 });
await pg.click(".set-tab:has-text('áudio')");
await pg.waitForSelector(".set-body select", { timeout: 10000 });
const info = await pg.evaluate(() => {
  const sels = [...document.querySelectorAll(".set-body select")];
  return {
    quantos: sels.length,
    entradas: sels[0] ? [...sels[0].options].map(o => o.textContent) : [],
    temSaida: !!sels[1],
    textoSaida: document.body.textContent.includes("não deixa o app escolher a saída"),
  };
});
ok(info.entradas.length >= 2, `microfones listados: ${JSON.stringify(info.entradas)}`);
ok(info.entradas[0] === "padrão do sistema", "primeira opção é o padrão do sistema");
ok(info.temSaida || info.textoSaida, info.temSaida
  ? "alto-falante também é escolhível neste sistema"
  : "sistema sem setSinkId: o app avisa em vez de fingir que dá");

console.log("\n→ toggles de processamento existem e persistem");
await pg.evaluate(() => {
  const linhas = [...document.querySelectorAll(".set-row")];
  const eco = linhas.find(l => l.textContent.includes("Cancelamento de eco"));
  eco.querySelector(".seg-btn").click(); // off
});
await pg.waitForTimeout(200);
const salvo = await pg.evaluate(() => JSON.parse(localStorage.getItem("bc_tweaks") || "{}").cancelamentoEco);
ok(salvo === false, `desligar o cancelamento de eco persiste (veio ${salvo})`);

console.log("\n→ ABRIR CONFIGURAÇÕES NÃO PODE DERRUBAR A CHAMADA");
await pg.click(".appbar .btn-ghost");
await pg.waitForSelector(".app-shell", { timeout: 10000 });
await pg.evaluate(() => [...document.querySelectorAll(".canal")].find(e => e.querySelector(".hash")?.textContent === "🔊").click());
await pg.waitForSelector("button:has-text('Entrar na call')", { timeout: 10000 });
await pg.click("button:has-text('Entrar na call')");
await pg.waitForSelector(".voz-bar", { timeout: 15000 });
ok(true, "entrou na call");

await pg.click(".rail-item[title='Configurações']");
await pg.waitForSelector(".config-overlay", { timeout: 10000 });
ok(await pg.isVisible(".voz-bar"), "a barra da call continua viva com as configurações abertas");

await pg.click(".set-tab:has-text('áudio')");
await pg.waitForSelector(".set-body select", { timeout: 10000 });
const trocou = await pg.evaluate(async () => {
  const sel = document.querySelector(".set-body select");
  const outra = [...sel.options].find(o => o.value !== "default");
  if (!outra) return "sem-outro-dispositivo";
  const setter = Object.getOwnPropertyDescriptor(window.HTMLSelectElement.prototype, "value").set;
  setter.call(sel, outra.value);
  sel.dispatchEvent(new Event("change", { bubbles: true }));
  return outra.value;
});
await pg.waitForTimeout(1500);
ok(trocou !== "sem-outro-dispositivo", `trocou o microfone durante a call (${String(trocou).slice(0, 12)}…)`);
ok(await pg.isVisible(".voz-bar"), "a call SOBREVIVEU à troca de microfone");
ok(erros.length === 0, `sem erro de página no fluxo todo${erros.length ? ": " + erros[0] : ""}`);

await pg.click(".appbar .btn-ghost");
ok(await pg.isVisible(".voz-bar"), "e continua viva depois de fechar as configurações");

await browser.close();
console.log(falhas === 0 ? "\nTUDO VERDE" : `\n${falhas} FALHA(S)`);
process.exit(falhas ? 1 : 0);

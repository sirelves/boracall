// Smoke das preferências que afetam a chamada: push-to-talk e densidade.
//
// Existe porque as três opções (PTT, densidade e o esmaecer da interface)
// continuaram aparecendo no painel depois da reescrita do front, mas pararam de
// fazer efeito — e nada no repositório percebeu. Quem encontrou foi um usuário
// (issue #43), não o projeto.
//
// Cobre também dois casos que a versão anterior não tratava e que só existem
// agora que há canais de texto:
//   - espaço digitado no chat NÃO pode abrir o microfone
//   - perder o foco com a tecla apertada tem que fechar o microfone
//
// Pré-requisitos:
//   cargo run -p boracall-server
//   (cd dist && python3 -m http.server 5174)
//   npm i playwright && npx playwright install chromium
//
// Uso: node scripts/smoke-preferencias.mjs
import { chromium } from "playwright";

const API = "http://127.0.0.1:3030";
const APP = "http://127.0.0.1:5174/index.html";
let falhas = 0;
const ok = (c, m) => { console.log(`${c ? "  ok  " : " FALHA"} ${m}`); if (!c) falhas++; };

const j = async (p, o = {}) => {
  const r = await fetch(API + p, {
    method: o.method || "GET",
    headers: { "content-type": "application/json", ...(o.token ? { authorization: "Bearer " + o.token } : {}) },
    body: o.body ? JSON.stringify(o.body) : undefined,
  });
  const t = await r.text();
  if (!r.ok) throw new Error(`${p} ${r.status} ${t}`);
  return t ? JSON.parse(t) : null;
};
const rnd = () => Math.random().toString(36).slice(2, 10);

const u = await j("/api/auth/signup", { method: "POST", body: { email: `ptt-${rnd()}@teste.local`, password: "senha-de-teste-123", display_name: "Ana" } });
const srv = await j("/api/servers", { method: "POST", token: u.token, body: { name: "Teste PTT" } });
const voz = srv.channels.find(c => c.kind === "voice");
const texto = srv.channels.find(c => c.kind === "text");

const browser = await chromium.launch({ args: ["--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream"] });
const ctx = await browser.newContext({ permissions: ["microphone"] });
const pg = await ctx.newPage();
const erros = [];
pg.on("pageerror", e => erros.push(e.message));

// entra já com o modo "segurar pra falar" ligado
await pg.addInitScript(([t, us]) => {
  localStorage.setItem("bc_token", t);
  localStorage.setItem("bc_user", us);
  localStorage.setItem("bc_route", JSON.stringify("app"));
  localStorage.setItem("bc_tweaks", JSON.stringify({ micMode: "ptt", density: "comfy", invisible: false }));
}, [u.token, JSON.stringify(u.user)]);

await pg.goto(APP);
await pg.waitForSelector(".app-shell", { timeout: 15000 });
// a lista de canais chega por fetch depois do shell
await pg.waitForFunction(
  () => [...document.querySelectorAll(".canal .hash")].some(e => e.textContent === "🔊"),
  { timeout: 15000 });
ok(erros.length === 0, `sem erro de página${erros.length ? ": " + erros[0] : ""}`);

// --- entra no canal de voz ---
await pg.evaluate(() => {
  [...document.querySelectorAll(".canal")].find(el => el.querySelector(".hash")?.textContent === "🔊").click();
});
await pg.waitForSelector("text=Entrar na call", { timeout: 10000 });
await pg.click("text=Entrar na call");
await pg.waitForSelector(".voz-bar", { timeout: 15000 });

const estado = () => pg.evaluate(() => ({
  transmitindo: !!document.querySelector(".transmit"),
  botao: document.querySelector(".voz-bar .fbtn")?.textContent?.trim() || "",
  pttAtivo: !!document.querySelector(".fbtn-ptt.ptt-active"),
}));

console.log("\n→ ao entrar no modo segurar, o microfone começa FECHADO");
let e0 = await estado();
ok(!e0.transmitindo, "sem faixa TRANSMITINDO");
ok(/[Ss]egure pra falar/.test(e0.botao), `botão diz "segure pra falar" (veio: "${e0.botao}")`);

console.log("\n→ segurando espaço");
await pg.keyboard.down("Space");
await pg.waitForTimeout(300);
let e1 = await estado();
ok(e1.transmitindo, "faixa TRANSMITINDO aparece");
ok(e1.pttAtivo, "botão fica destacado");

console.log("\n→ soltando");
await pg.keyboard.up("Space");
await pg.waitForTimeout(300);
let e2 = await estado();
ok(!e2.transmitindo, "faixa some ao soltar");
ok(!e2.pttAtivo, "botão volta ao normal");

console.log("\n→ espaço enquanto escreve no chat NÃO pode transmitir");
await pg.evaluate(() => {
  [...document.querySelectorAll(".canal")].find(el => el.querySelector(".hash")?.textContent === "#").click();
});
await pg.waitForSelector(".composer input", { timeout: 10000 });
await pg.click(".composer input");
await pg.keyboard.type("oi mundo");
await pg.waitForTimeout(200);
const e3 = await estado();
ok(!e3.transmitindo, "digitar espaço no chat não abre o microfone");
const escrito = await pg.inputValue(".composer input");
ok(escrito === "oi mundo", `o espaço foi pro texto (veio: "${escrito}")`);

console.log("\n→ perder o foco com a tecla apertada fecha o microfone");
await pg.evaluate(() => document.activeElement.blur());
await pg.evaluate(() => {
  const alvo = [...document.querySelectorAll(".canal")].find(el => el.querySelector(".hash")?.textContent === "🔊");
  alvo.click();
});
await pg.waitForTimeout(300);
await pg.keyboard.down("Space");
await pg.waitForTimeout(200);
ok((await estado()).transmitindo, "transmitindo antes do blur");
await pg.evaluate(() => window.dispatchEvent(new Event("blur")));
await pg.waitForTimeout(300);
ok(!(await estado()).transmitindo, "blur com a tecla apertada fecha o microfone");
await pg.keyboard.up("Space");

console.log("\n→ densidade muda a interface de verdade");
// A preferência foi salva como "comfy" antes de abrir o app: quem tem que
// aplicar a classe é o app, não o teste.
ok(await pg.evaluate(() => document.querySelector(".app-shell").classList.contains("comfy")),
   "o app aplicou a classe da preferência salva (comfy)");

// Trocar a preferência pela própria UI tem que mudar a interface — e o efeito
// precisa ser mensurável, não basta a classe trocar.
await pg.evaluate(() => {
  [...document.querySelectorAll(".canal")].find(el => el.querySelector(".hash")?.textContent === "#").click();
});
// precisa existir mensagem na tela pra medir o efeito da densidade
await pg.click(".composer input");
await pg.keyboard.type("mensagem pra medir densidade");
await pg.keyboard.press("Enter");
await pg.waitForSelector(".msg-texto", { timeout: 10000 });
const antesPx = await pg.evaluate(() => getComputedStyle(document.querySelector(".msg-texto")).fontSize);

// Configurações → preferências → compacta
await pg.click(".rail-item[title='Configurações']");
await pg.waitForSelector(".set-tabs", { timeout: 10000 });
await pg.click("text=preferências");
await pg.click(".tweaks .seg-btn:has-text('compacta')");
await pg.click("text=← voltar");
await pg.waitForSelector(".app-shell.compact", { timeout: 10000 });
ok(true, "escolher 'compacta' na tela de preferências aplica a classe");

await pg.waitForFunction(
  () => [...document.querySelectorAll(".canal .hash")].some(e => e.textContent === "#"),
  { timeout: 15000 });
await pg.evaluate(() => {
  [...document.querySelectorAll(".canal")].find(el => el.querySelector(".hash")?.textContent === "#").click();
});
await pg.waitForSelector(".msg-texto", { timeout: 10000 });
const depoisPx = await pg.evaluate(() => getComputedStyle(document.querySelector(".msg-texto")).fontSize);
ok(antesPx !== depoisPx,
   `o texto da mensagem mudou de tamanho de fato (${antesPx} → ${depoisPx})`);

await browser.close();
console.log(falhas === 0 ? "\nTUDO VERDE" : `\n${falhas} FALHA(S)`);
process.exit(falhas ? 1 : 0);

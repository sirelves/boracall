// BoraCall — configuração de runtime.
//
// O padrão é o backend LOCAL. O build de produção sobrescreve este arquivo
// inteiro (passo "Bake production env.js" no .github/workflows/release.yml)
// antes de empacotar, então o valor daqui nunca chega no bundle publicado.
//
// Já foi o contrário — este arquivo vinha apontando pra api.boracall.com — e o
// efeito era que abrir o app em desenvolvimento mandava cadastro e mensagem pro
// banco de produção sem nenhum aviso.
(function () {
  window.BC_API_URL    = window.BC_API_URL    || "http://127.0.0.1:3030";
  window.BC_PUBLIC_URL = window.BC_PUBLIC_URL || "http://127.0.0.1:3030";
})();

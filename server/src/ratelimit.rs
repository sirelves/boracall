//! Limite de requisições por chave, com balde de fichas (token bucket).
//!
//! Duas chaves diferentes, porque protegem coisas diferentes:
//!
//! - **por IP**, nas rotas de autenticação: contém força bruta de senha e
//!   criação em massa de conta.
//! - **por e-mail de destino**, nas rotas que MANDAM e-mail: um limite por IP
//!   não impede alguém de trocar de IP e bombardear a caixa de uma vítima —
//!   nem de queimar a cota do Resend e a reputação do domínio.
//!
//! Balde de fichas em vez de janela fixa porque janela fixa deixa passar o
//! dobro na virada (o pico do fim de uma janela emenda no começo da seguinte).
//!
//! Estado em memória, por processo. Para vários nós isso vira um limite por nó
//! em vez de global — aceitável para um controle que é dissuasivo, não contábil.
//! Se virar problema, o mesmo formato de chave migra pra Redis.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct Politica {
    /// Quantas requisições em rajada são toleradas.
    pub burst: u32,
    /// Em quanto tempo o balde reenche por completo.
    pub janela: Duration,
}

impl Politica {
    pub const fn new(burst: u32, janela: Duration) -> Self {
        Self { burst, janela }
    }
    /// Fichas recuperadas por segundo.
    fn taxa(&self) -> f64 {
        self.burst as f64 / self.janela.as_secs_f64()
    }
}

struct Balde {
    fichas: f64,
    visto_em: Instant,
}

#[derive(Clone)]
pub struct RateLimiter {
    baldes: Arc<DashMap<String, Balde>>,
    politica: Politica,
}

/// Quanto esperar até a próxima ficha, quando negado.
#[derive(Debug)]
pub struct Negado {
    pub retry_after: Duration,
}

impl RateLimiter {
    pub fn new(politica: Politica) -> Self {
        Self {
            baldes: Arc::new(DashMap::new()),
            politica,
        }
    }

    /// Consome uma ficha da chave. `Err` quando não há ficha disponível.
    pub fn checar(&self, chave: &str) -> Result<(), Negado> {
        self.checar_em(chave, Instant::now())
    }

    /// Igual ao `checar`, com o relógio injetado — é o que torna o
    /// comportamento no tempo testável sem `sleep` na suíte.
    fn checar_em(&self, chave: &str, agora: Instant) -> Result<(), Negado> {
        let taxa = self.politica.taxa();
        let teto = self.politica.burst as f64;

        let mut balde = self.baldes.entry(chave.to_string()).or_insert(Balde {
            fichas: teto,
            visto_em: agora,
        });

        // Reenche pelo tempo decorrido, sem passar do teto.
        let decorrido = agora
            .saturating_duration_since(balde.visto_em)
            .as_secs_f64();
        balde.fichas = (balde.fichas + decorrido * taxa).min(teto);
        balde.visto_em = agora;

        if balde.fichas >= 1.0 {
            balde.fichas -= 1.0;
            Ok(())
        } else {
            let faltando = 1.0 - balde.fichas;
            Err(Negado {
                retry_after: Duration::from_secs_f64((faltando / taxa).ceil().max(1.0)),
            })
        }
    }

    /// Descarta baldes cheios e parados. Sem isso o mapa cresce com uma entrada
    /// por IP que já apareceu uma vez — memória que nunca volta.
    pub fn limpar(&self) {
        self.limpar_em(Instant::now());
    }

    fn limpar_em(&self, agora: Instant) {
        let teto = self.politica.burst as f64;
        let taxa = self.politica.taxa();
        self.baldes.retain(|_, b| {
            let decorrido = agora.saturating_duration_since(b.visto_em).as_secs_f64();
            (b.fichas + decorrido * taxa) < teto
        });
    }

    #[cfg(test)]
    fn tamanho(&self) -> usize {
        self.baldes.len()
    }
}

// ---------------------------------------------------------------------------
// Integração com o axum
// ---------------------------------------------------------------------------

use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// IP real de quem chamou.
///
/// Confia em `X-Real-IP` e NÃO em `X-Forwarded-For`: o nginx sobrescreve o
/// primeiro com o endereço resolvido (que já é o `CF-Connecting-IP`, por causa
/// do `real_ip_header`), enquanto o segundo carrega junto o que o cliente
/// mandou — dá pra forjar e furar o limite.
///
/// Sem o header, cai no endereço do socket. Em produção isso seria o próprio
/// nginx, mas aí não há proxy nenhum: é chamada local, em desenvolvimento.
pub fn ip_de(headers: &HeaderMap, socket: Option<std::net::SocketAddr>) -> String {
    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            socket
                .map(|a| a.ip().to_string())
                .unwrap_or_else(|| "desconhecido".into())
        })
}

fn resposta_negada(negado: Negado) -> Response {
    AppError::RateLimited {
        retry_after_secs: negado.retry_after.as_secs().max(1),
    }
    .into_response()
}

/// Camada por IP, aplicada nas rotas de autenticação.
pub async fn por_ip(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let socket = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0);
    let ip = ip_de(req.headers(), socket);

    match state.limite_ip.checar(&ip) {
        Ok(()) => next.run(req).await,
        Err(negado) => {
            tracing::warn!(%ip, "limite por IP atingido em rota de auth");
            resposta_negada(negado)
        }
    }
}

/// Checagem por e-mail de destino, chamada de dentro dos handlers que enviam
/// mensagem. Fica no handler e não numa camada porque só ali se sabe pra quem
/// o e-mail vai — o endereço está no corpo, não na URL.
pub fn checar_envio(state: &AppState, email: &str) -> AppResult<()> {
    state
        .limite_email
        .checar(&email.trim().to_lowercase())
        .map_err(|negado| {
            tracing::warn!(%email, "limite de envio de e-mail atingido");
            AppError::RateLimited {
                retry_after_secs: negado.retry_after.as_secs().max(1),
            }
        })
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const UM_MIN: Duration = Duration::from_secs(60);

    #[test]
    fn deixa_passar_ate_o_burst_e_barra_o_seguinte() {
        let rl = RateLimiter::new(Politica::new(3, UM_MIN));
        let t = Instant::now();

        for i in 1..=3 {
            assert!(rl.checar_em("ip", t).is_ok(), "requisição {i} devia passar");
        }
        assert!(rl.checar_em("ip", t).is_err(), "a quarta devia ser barrada");
    }

    #[test]
    fn chaves_diferentes_nao_se_atrapalham() {
        // Um IP abusando não pode derrubar o resto do mundo.
        let rl = RateLimiter::new(Politica::new(1, UM_MIN));
        let t = Instant::now();

        assert!(rl.checar_em("ip-a", t).is_ok());
        assert!(rl.checar_em("ip-a", t).is_err(), "A esgotou");
        assert!(rl.checar_em("ip-b", t).is_ok(), "B tem balde próprio");
    }

    #[test]
    fn reenche_com_o_passar_do_tempo() {
        let rl = RateLimiter::new(Politica::new(6, UM_MIN)); // 1 ficha a cada 10s
        let t0 = Instant::now();

        for _ in 0..6 {
            assert!(rl.checar_em("ip", t0).is_ok());
        }
        assert!(rl.checar_em("ip", t0).is_err());

        // 10s depois: exatamente uma ficha de volta.
        let t1 = t0 + Duration::from_secs(10);
        assert!(rl.checar_em("ip", t1).is_ok(), "devia ter reenchido uma");
        assert!(rl.checar_em("ip", t1).is_err(), "só uma, não duas");
    }

    #[test]
    fn nao_acumula_alem_do_burst() {
        // Ficar uma hora quieto não dá direito a uma hora de rajada.
        let rl = RateLimiter::new(Politica::new(2, UM_MIN));
        let t0 = Instant::now();
        assert!(rl.checar_em("ip", t0).is_ok());

        let t1 = t0 + Duration::from_secs(3600);
        assert!(rl.checar_em("ip", t1).is_ok());
        assert!(rl.checar_em("ip", t1).is_ok());
        assert!(
            rl.checar_em("ip", t1).is_err(),
            "o balde não pode passar do teto de {}",
            2
        );
    }

    #[test]
    fn retry_after_e_positivo_e_plausivel() {
        let rl = RateLimiter::new(Politica::new(1, Duration::from_secs(10)));
        let t = Instant::now();
        assert!(rl.checar_em("ip", t).is_ok());

        let negado = rl.checar_em("ip", t).expect_err("devia negar");
        assert!(negado.retry_after >= Duration::from_secs(1));
        assert!(
            negado.retry_after <= Duration::from_secs(10),
            "esperar mais que a janela inteira não faz sentido: {:?}",
            negado.retry_after
        );
    }

    #[test]
    fn limpeza_descarta_balde_ja_reenchido() {
        let rl = RateLimiter::new(Politica::new(2, UM_MIN));
        let t0 = Instant::now();

        rl.checar_em("passou-por-aqui", t0).unwrap();
        assert_eq!(rl.tamanho(), 1);

        // Um minuto depois o balde está cheio de novo: guardar a entrada só
        // gasta memória.
        rl.limpar_em(t0 + UM_MIN);
        assert_eq!(rl.tamanho(), 0, "balde cheio e parado não precisa ficar");
    }

    #[test]
    fn limpeza_mantem_quem_ainda_esta_limitado() {
        let rl = RateLimiter::new(Politica::new(1, Duration::from_secs(3600)));
        let t0 = Instant::now();
        rl.checar_em("abusador", t0).unwrap();

        // Esgotou e leva uma hora pra voltar. Esquecer a entrada agora zeraria
        // o castigo — bastaria esperar o ciclo de limpeza pra continuar.
        rl.limpar_em(t0 + Duration::from_secs(60));
        assert_eq!(rl.tamanho(), 1);
        assert!(rl
            .checar_em("abusador", t0 + Duration::from_secs(60))
            .is_err());
    }
}

//! `GET /api/ice` — servidores ICE (STUN e TURN) pro cliente montar a mesh.
//!
//! Por que isso é um endpoint e não uma constante no bundle: a credencial de
//! TURN é **efêmera**. Usuário e senha fixos embutidos no app seriam extraídos
//! do binário em cinco minutos, e aí qualquer um usa teu relay de graça.
//!
//! O mecanismo é o `use-auth-secret` do coturn (o mesmo do "TURN REST API"):
//!
//!   username   = "<unix-de-validade>:<id-do-usuario>"
//!   credential = base64( HMAC-SHA1( segredo, username ) )
//!
//! O coturn valida com o mesmo segredo, sem precisar de banco de usuários e sem
//! nenhuma chamada entre os dois serviços. Trocar o segredo invalida tudo.
//!
//! Um efeito colateral bom: dá pra rotacionar credencial sem publicar versão
//! nova do desktop, porque quem decide é o servidor.

use axum::{extract::State, Json};
use base64::Engine;
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha1::Sha1;

use crate::auth::AuthUser;
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IceConfig {
    pub ice_servers: Vec<IceServer>,
    /// Segundos até a credencial expirar. O cliente pode buscar de novo antes.
    pub ttl: i64,
}

/// Gera a credencial no formato que o coturn espera.
fn credencial_efemera(secret: &str, user_id: &str, ttl: i64) -> (String, String) {
    let validade = Utc::now().timestamp() + ttl;
    let username = format!("{validade}:{user_id}");

    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(secret.as_bytes())
        .expect("HMAC aceita chave de qualquer tamanho");
    mac.update(username.as_bytes());
    let credential = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    (username, credential)
}

pub async fn ice_servers(
    auth: AuthUser,
    State(state): State<AppState>,
) -> AppResult<Json<IceConfig>> {
    let mut servers = Vec::new();

    if !state.stun_urls.is_empty() {
        servers.push(IceServer {
            urls: state.stun_urls.clone(),
            username: None,
            credential: None,
        });
    }

    // Sem segredo configurado, devolve só STUN. É o modo de desenvolvimento —
    // e é honesto: melhor o cliente saber que não há relay do que receber uma
    // credencial que o coturn vai recusar.
    match (&state.turn_secret, state.turn_urls.is_empty()) {
        (Some(secret), false) => {
            let (username, credential) =
                credencial_efemera(secret, &auth.id.to_string(), state.turn_ttl_secs);
            servers.push(IceServer {
                urls: state.turn_urls.clone(),
                username: Some(username),
                credential: Some(credential),
            });
        }
        _ => {
            tracing::debug!("TURN não configurado — devolvendo só STUN");
        }
    }

    Ok(Json(IceConfig {
        ice_servers: servers,
        ttl: state.turn_ttl_secs,
    }))
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SEGREDO: &str = "segredo-de-teste";

    #[test]
    fn username_carrega_validade_no_futuro_e_o_id() {
        let (username, _) = credencial_efemera(SEGREDO, "user-123", 3600);
        let (validade, id) = username.split_once(':').expect("formato <validade>:<id>");

        assert_eq!(id, "user-123");
        let validade: i64 = validade.parse().expect("validade é unix timestamp");
        let agora = Utc::now().timestamp();
        assert!(validade > agora, "credencial já nasce expirada");
        assert!(
            validade <= agora + 3601,
            "validade muito no futuro: {validade}"
        );
    }

    #[test]
    fn credencial_e_o_hmac_sha1_do_username_em_base64() {
        let (username, credential) = credencial_efemera(SEGREDO, "user-123", 3600);

        // Recalcula do zero: é exatamente essa conta que o coturn refaz do lado dele.
        let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(SEGREDO.as_bytes()).unwrap();
        mac.update(username.as_bytes());
        let esperado =
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

        assert_eq!(credential, esperado);
    }

    #[test]
    fn segredo_diferente_gera_credencial_diferente() {
        // Trocar o segredo no coturn precisa invalidar o que já foi entregue.
        let (u1, c1) = credencial_efemera(SEGREDO, "user-123", 3600);
        let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(b"outro-segredo").unwrap();
        mac.update(u1.as_bytes());
        let c2 = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

        assert_ne!(c1, c2);
    }

    #[test]
    fn usuarios_diferentes_recebem_credenciais_diferentes() {
        // Credencial por usuário é o que permite cortar um abusador sozinho.
        let (u1, c1) = credencial_efemera(SEGREDO, "user-aaa", 3600);
        let (u2, c2) = credencial_efemera(SEGREDO, "user-bbb", 3600);
        assert_ne!(u1, u2);
        assert_ne!(c1, c2);
    }

    #[test]
    fn credencial_nao_e_o_segredo_em_lugar_nenhum() {
        // Falha óbvia de implementação: mandar o static-auth-secret pro cliente.
        let (username, credential) = credencial_efemera(SEGREDO, "user-123", 3600);
        assert!(!username.contains(SEGREDO));
        assert!(!credential.contains(SEGREDO));
    }
}

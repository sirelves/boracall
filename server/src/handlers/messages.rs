//! Mensagens de canal de texto: histórico, envio, edição, remoção e leitura.
//!
//! Paginação é por cursor (`before=<id>`), nunca por offset: com offset, uma
//! mensagem nova chegando durante a rolagem empurra a janela e o usuário vê
//! item repetido ou pula item.
//!
//! Como o id é UUID v4 (aleatório, não ordena por tempo), o cursor é o par
//! `(created_at, id)` — comparado como row value, que o Postgres resolve pelo
//! índice `messages_channel_cursor_idx`.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const MAX_BODY: usize = 4000;
const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 100;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct MessageDto {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub user_id: Uuid,
    pub display_name: Option<String>,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct MessagePage {
    /// Mais novas primeiro. O front inverte pra renderizar.
    pub messages: Vec<MessageDto>,
    /// Cursor pra próxima página (mais antigas). `None` = chegou no começo.
    pub next_before: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub before: Option<Uuid>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct SendMessageReq {
    #[validate(length(min = 1, max = 4000))]
    pub body: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditMessageReq {
    #[validate(length(min = 1, max = 4000))]
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct MarkReadReq {
    /// Até qual mensagem foi lido. Ausente = marca tudo até agora.
    pub message_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct ReadStateDto {
    pub channel_id: Uuid,
    pub last_read_at: DateTime<Utc>,
    pub unread: i64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct ChannelCtx {
    id: Uuid,
    kind: String,
}

/// Resolve o canal pelo slug e exige que quem pediu seja membro do servidor dono.
///
/// 404 (e não 403) pra quem não é membro: confirmar existência de canal
/// permitiria varrer slugs e mapear o que existe no sistema.
async fn channel_for_member(state: &AppState, slug: &str, user_id: Uuid) -> AppResult<ChannelCtx> {
    let row = sqlx::query!(
        r#"
        SELECT c.id, c.kind, (m.user_id IS NOT NULL) AS "is_member!"
        FROM channels c
        JOIN servers s ON s.id = c.server_id
        LEFT JOIN server_members m ON m.server_id = s.id AND m.user_id = $2
        WHERE c.slug = $1
        "#,
        slug,
        user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    if !row.is_member {
        return Err(AppError::NotFound);
    }
    Ok(ChannelCtx {
        id: row.id,
        kind: row.kind,
    })
}

/// Canal de voz não guarda mensagem — quem tentar recebe erro explicado.
fn require_text(ch: &ChannelCtx) -> AppResult<()> {
    if ch.kind != "text" {
        return Err(AppError::Validation(
            "canal de voz não recebe mensagem".into(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Histórico do canal, mais novas primeiro, paginado por cursor.
pub async fn list_messages(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> AppResult<Json<MessagePage>> {
    let ch = channel_for_member(&state, &slug, auth.id).await?;
    require_text(&ch)?;

    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    // Busca uma a mais que o pedido pra saber se existe página seguinte sem
    // precisar de um COUNT separado.
    let fetch = limit + 1;

    let mut rows = match q.before {
        // Cursor: tudo estritamente mais antigo que a mensagem informada.
        // A subquery resolve o (created_at, id) do cursor; se o id não existir
        // no canal, a comparação não casa com nada e a página volta vazia.
        Some(before) => sqlx::query!(
            r#"
                SELECT m.id, m.channel_id, m.user_id, m.body, m.created_at, m.edited_at,
                       u.display_name
                FROM messages m
                JOIN users u ON u.id = m.user_id
                WHERE m.channel_id = $1
                  AND (m.created_at, m.id) < (
                        SELECT created_at, id FROM messages WHERE id = $2 AND channel_id = $1
                  )
                ORDER BY m.created_at DESC, m.id DESC
                LIMIT $3
                "#,
            ch.id,
            before,
            fetch
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| MessageDto {
            id: r.id,
            channel_id: r.channel_id,
            user_id: r.user_id,
            display_name: r.display_name,
            body: r.body,
            created_at: r.created_at,
            edited_at: r.edited_at,
        })
        .collect::<Vec<_>>(),
        None => sqlx::query!(
            r#"
            SELECT m.id, m.channel_id, m.user_id, m.body, m.created_at, m.edited_at,
                   u.display_name
            FROM messages m
            JOIN users u ON u.id = m.user_id
            WHERE m.channel_id = $1
            ORDER BY m.created_at DESC, m.id DESC
            LIMIT $2
            "#,
            ch.id,
            fetch
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| MessageDto {
            id: r.id,
            channel_id: r.channel_id,
            user_id: r.user_id,
            display_name: r.display_name,
            body: r.body,
            created_at: r.created_at,
            edited_at: r.edited_at,
        })
        .collect::<Vec<_>>(),
    };

    // A extra só serviu pra detectar que há mais — não vai na resposta.
    let next_before = if rows.len() as i64 > limit {
        rows.truncate(limit as usize);
        rows.last().map(|m| m.id)
    } else {
        None
    };

    Ok(Json(MessagePage {
        messages: rows,
        next_before,
    }))
}

/// Envia mensagem no canal.
pub async fn send_message(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(body): Json<SendMessageReq>,
) -> AppResult<Json<MessageDto>> {
    body.validate()?;
    let ch = channel_for_member(&state, &slug, auth.id).await?;
    require_text(&ch)?;

    let text = body.body.trim().to_string();
    if text.is_empty() {
        return Err(AppError::Validation("mensagem vazia".into()));
    }
    // A validação acima conta chars do original; depois do trim o limite continua
    // valendo, então não há caminho pra passar batido.
    debug_assert!(text.chars().count() <= MAX_BODY);

    let row = sqlx::query!(
        r#"
        WITH inserted AS (
            INSERT INTO messages (channel_id, user_id, body)
            VALUES ($1, $2, $3)
            RETURNING id, channel_id, user_id, body, created_at, edited_at
        )
        SELECT i.id, i.channel_id, i.user_id, i.body, i.created_at, i.edited_at,
               u.display_name
        FROM inserted i
        JOIN users u ON u.id = i.user_id
        "#,
        ch.id,
        auth.id,
        text
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(MessageDto {
        id: row.id,
        channel_id: row.channel_id,
        user_id: row.user_id,
        display_name: row.display_name,
        body: row.body,
        created_at: row.created_at,
        edited_at: row.edited_at,
    }))
}

/// Edita mensagem. Só o autor.
pub async fn edit_message(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<EditMessageReq>,
) -> AppResult<Json<MessageDto>> {
    body.validate()?;
    let text = body.body.trim().to_string();
    if text.is_empty() {
        return Err(AppError::Validation("mensagem vazia".into()));
    }

    let existing = sqlx::query!("SELECT user_id FROM messages WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if existing.user_id != auth.id {
        return Err(AppError::Forbidden("só o autor edita a mensagem".into()));
    }

    let row = sqlx::query!(
        r#"
        WITH updated AS (
            UPDATE messages SET body = $2, edited_at = NOW()
            WHERE id = $1
            RETURNING id, channel_id, user_id, body, created_at, edited_at
        )
        SELECT u2.id, u2.channel_id, u2.user_id, u2.body, u2.created_at, u2.edited_at,
               us.display_name
        FROM updated u2
        JOIN users us ON us.id = u2.user_id
        "#,
        id,
        text
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(MessageDto {
        id: row.id,
        channel_id: row.channel_id,
        user_id: row.user_id,
        display_name: row.display_name,
        body: row.body,
        created_at: row.created_at,
        edited_at: row.edited_at,
    }))
}

/// Apaga mensagem. Autor ou dono do servidor (moderação).
pub async fn delete_message(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let row = sqlx::query!(
        r#"
        SELECT m.user_id, s.owner_id
        FROM messages m
        JOIN channels c ON c.id = m.channel_id
        JOIN servers s ON s.id = c.server_id
        WHERE m.id = $1
        "#,
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    if row.user_id != auth.id && row.owner_id != auth.id {
        return Err(AppError::Forbidden(
            "só o autor ou o dono do servidor apaga".into(),
        ));
    }

    sqlx::query!("DELETE FROM messages WHERE id = $1", id)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// Marca o canal como lido até uma mensagem (ou até agora, se não informar).
pub async fn mark_read(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(body): Json<MarkReadReq>,
) -> AppResult<Json<ReadStateDto>> {
    let ch = channel_for_member(&state, &slug, auth.id).await?;
    require_text(&ch)?;

    // Timestamp do marcador: o da mensagem informada, ou `None` pra deixar o
    // banco carimbar com NOW().
    //
    // O relógio precisa ser SEMPRE o do banco. As mensagens nascem com NOW() do
    // Postgres; se o marcador viesse de Utc::now() do processo, qualquer skew de
    // NTP entre app e banco (centésimos de segundo bastam) deixaria mensagens
    // "no futuro" — o usuário clica em marcar lido e o não-lido não zera.
    let last_read_at = match body.message_id {
        Some(mid) => Some(
            sqlx::query_scalar!(
                "SELECT created_at FROM messages WHERE id = $1 AND channel_id = $2",
                mid,
                ch.id
            )
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?,
        ),
        None => None,
    };

    // Marcador só anda pra frente: um "marcar lido" atrasado chegando fora de
    // ordem não pode ressuscitar não-lidos que o usuário já viu.
    let saved = sqlx::query!(
        r#"
        INSERT INTO message_reads (channel_id, user_id, last_read_message_id, last_read_at)
        VALUES ($1, $2, $3, COALESCE($4, NOW()))
        ON CONFLICT (channel_id, user_id) DO UPDATE
            SET last_read_message_id = CASE
                    WHEN EXCLUDED.last_read_at > message_reads.last_read_at
                    THEN EXCLUDED.last_read_message_id
                    ELSE message_reads.last_read_message_id
                END,
                last_read_at = GREATEST(message_reads.last_read_at, EXCLUDED.last_read_at)
        RETURNING last_read_at
        "#,
        ch.id,
        auth.id,
        body.message_id,
        last_read_at
    )
    .fetch_one(&state.db)
    .await?;

    let unread = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "c!" FROM messages
           WHERE channel_id = $1 AND created_at > $2 AND user_id <> $3"#,
        ch.id,
        saved.last_read_at,
        auth.id
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(ReadStateDto {
        channel_id: ch.id,
        last_read_at: saved.last_read_at,
        unread,
    }))
}

// ---------------------------------------------------------------------------
// Testes de integração — precisam de Postgres, ver handlers/servers.rs.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::servers::tests::{make_user, test_state};
    use crate::handlers::servers::{
        create_server, get_server, join_server, ChannelDto, CreateServerReq,
    };

    /// Cria servidor e devolve (dono, slug do servidor, canal de texto, canal de voz).
    async fn setup(state: &AppState) -> (AuthUser, String, ChannelDto, ChannelDto) {
        let dono = make_user(state).await;
        let out = create_server(
            dono.clone(),
            State(state.clone()),
            Json(CreateServerReq {
                name: "Servidor de teste".into(),
            }),
        )
        .await
        .unwrap()
        .0;

        let texto = out
            .channels
            .iter()
            .find(|c| c.kind == "text")
            .unwrap()
            .clone();
        let voz = out
            .channels
            .iter()
            .find(|c| c.kind == "voice")
            .unwrap()
            .clone();
        (dono, out.server.slug, texto, voz)
    }

    async fn send(state: &AppState, who: &AuthUser, slug: &str, body: &str) -> MessageDto {
        send_message(
            who.clone(),
            State(state.clone()),
            Path(slug.to_string()),
            Json(SendMessageReq { body: body.into() }),
        )
        .await
        .unwrap()
        .0
    }

    async fn history(
        state: &AppState,
        who: &AuthUser,
        slug: &str,
        before: Option<Uuid>,
        limit: Option<i64>,
    ) -> MessagePage {
        list_messages(
            who.clone(),
            State(state.clone()),
            Path(slug.to_string()),
            Query(HistoryQuery { before, limit }),
        )
        .await
        .unwrap()
        .0
    }

    #[tokio::test]
    #[ignore]
    async fn envia_e_le_de_volta_com_autor() {
        let state = test_state().await;
        let (dono, _s, texto, _v) = setup(&state).await;

        let m = send(&state, &dono, &texto.slug, "  bom dia  ").await;
        assert_eq!(m.body, "bom dia", "o corpo é trimado");
        assert_eq!(m.user_id, dono.id);
        assert!(m.edited_at.is_none());

        let page = history(&state, &dono, &texto.slug, None, None).await;
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.messages[0].id, m.id);
        assert!(page.next_before.is_none(), "só uma página");
    }

    #[tokio::test]
    #[ignore]
    async fn historico_vem_do_mais_novo_pro_mais_antigo() {
        let state = test_state().await;
        let (dono, _s, texto, _v) = setup(&state).await;

        for i in 0..5 {
            send(&state, &dono, &texto.slug, &format!("msg {i}")).await;
        }

        let page = history(&state, &dono, &texto.slug, None, None).await;
        let bodies: Vec<&str> = page.messages.iter().map(|m| m.body.as_str()).collect();
        assert_eq!(bodies, vec!["msg 4", "msg 3", "msg 2", "msg 1", "msg 0"]);
    }

    #[tokio::test]
    #[ignore]
    async fn paginacao_por_cursor_nao_repete_nem_pula() {
        let state = test_state().await;
        let (dono, _s, texto, _v) = setup(&state).await;

        for i in 0..10 {
            send(&state, &dono, &texto.slug, &format!("msg {i}")).await;
        }

        // Primeira página.
        let p1 = history(&state, &dono, &texto.slug, None, Some(4)).await;
        assert_eq!(p1.messages.len(), 4);
        let cursor = p1.next_before.expect("tem mais página");

        // Mensagem NOVA chegando entre uma página e outra. É exatamente aqui que
        // paginação por offset empurraria a janela e duplicaria item.
        send(&state, &dono, &texto.slug, "chegou agora").await;

        let p2 = history(&state, &dono, &texto.slug, Some(cursor), Some(4)).await;
        assert_eq!(p2.messages.len(), 4);

        let ids_p1: Vec<Uuid> = p1.messages.iter().map(|m| m.id).collect();
        for m in &p2.messages {
            assert!(
                !ids_p1.contains(&m.id),
                "mensagem {:?} repetiu entre páginas",
                m.body
            );
        }

        // Caminha até o fim e confere que viu as 10 originais, sem buraco.
        let mut vistos: Vec<String> = p1
            .messages
            .iter()
            .chain(p2.messages.iter())
            .map(|m| m.body.clone())
            .collect();
        let mut cur = p2.next_before;
        while let Some(c) = cur {
            let p = history(&state, &dono, &texto.slug, Some(c), Some(4)).await;
            vistos.extend(p.messages.iter().map(|m| m.body.clone()));
            cur = p.next_before;
        }
        for i in 0..10 {
            assert!(
                vistos.contains(&format!("msg {i}")),
                "faltou 'msg {i}' na varredura: {vistos:?}"
            );
        }
    }

    #[tokio::test]
    #[ignore]
    async fn ultima_pagina_nao_devolve_cursor() {
        let state = test_state().await;
        let (dono, _s, texto, _v) = setup(&state).await;

        for i in 0..3 {
            send(&state, &dono, &texto.slug, &format!("m{i}")).await;
        }

        // limit exatamente igual ao total: não pode sugerir que há mais.
        let page = history(&state, &dono, &texto.slug, None, Some(3)).await;
        assert_eq!(page.messages.len(), 3);
        assert!(
            page.next_before.is_none(),
            "não existe página seguinte, mas veio cursor"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn canal_de_voz_nao_recebe_mensagem() {
        let state = test_state().await;
        let (dono, _s, _t, voz) = setup(&state).await;

        let err = send_message(
            dono,
            State(state.clone()),
            Path(voz.slug),
            Json(SendMessageReq { body: "oi".into() }),
        )
        .await
        .expect_err("canal de voz não guarda mensagem");
        assert!(matches!(err, AppError::Validation(_)), "veio: {err:?}");
    }

    #[tokio::test]
    #[ignore]
    async fn quem_nao_e_membro_nao_le_nem_escreve() {
        let state = test_state().await;
        let (dono, _s, texto, _v) = setup(&state).await;
        send(&state, &dono, &texto.slug, "segredo").await;

        let estranho = make_user(&state).await;

        let err = list_messages(
            estranho.clone(),
            State(state.clone()),
            Path(texto.slug.clone()),
            Query(HistoryQuery {
                before: None,
                limit: None,
            }),
        )
        .await
        .expect_err("estranho não lê");
        assert!(matches!(err, AppError::NotFound), "veio: {err:?}");

        let err = send_message(
            estranho,
            State(state.clone()),
            Path(texto.slug),
            Json(SendMessageReq { body: "oi".into() }),
        )
        .await
        .expect_err("estranho não escreve");
        assert!(matches!(err, AppError::NotFound), "veio: {err:?}");
    }

    #[tokio::test]
    #[ignore]
    async fn so_o_autor_edita() {
        let state = test_state().await;
        let (dono, server_slug, texto, _v) = setup(&state).await;
        let outro = make_user(&state).await;
        let _ = join_server(
            outro.clone(),
            State(state.clone()),
            Path(server_slug.clone()),
        )
        .await
        .unwrap();

        let m = send(&state, &dono, &texto.slug, "original").await;

        let err = edit_message(
            outro,
            State(state.clone()),
            Path(m.id),
            Json(EditMessageReq {
                body: "sequestrada".into(),
            }),
        )
        .await
        .expect_err("membro comum não edita mensagem alheia");
        assert!(matches!(err, AppError::Forbidden(_)), "veio: {err:?}");

        let editada = edit_message(
            dono,
            State(state.clone()),
            Path(m.id),
            Json(EditMessageReq {
                body: "corrigida".into(),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(editada.body, "corrigida");
        assert!(
            editada.edited_at.is_some(),
            "edição precisa carimbar a data"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn dono_do_servidor_apaga_mensagem_alheia_mas_membro_nao() {
        let state = test_state().await;
        let (dono, server_slug, texto, _v) = setup(&state).await;

        let membro = make_user(&state).await;
        let _ = join_server(
            membro.clone(),
            State(state.clone()),
            Path(server_slug.clone()),
        )
        .await
        .unwrap();
        let intruso = make_user(&state).await;
        let _ = join_server(
            intruso.clone(),
            State(state.clone()),
            Path(server_slug.clone()),
        )
        .await
        .unwrap();

        let m = send(&state, &membro, &texto.slug, "mensagem do membro").await;

        // Outro membro qualquer não modera.
        let err = delete_message(intruso, State(state.clone()), Path(m.id))
            .await
            .expect_err("membro comum não apaga mensagem alheia");
        assert!(matches!(err, AppError::Forbidden(_)), "veio: {err:?}");

        // Dono do servidor modera.
        let _ = delete_message(dono, State(state.clone()), Path(m.id))
            .await
            .unwrap();

        let page = history(&state, &membro, &texto.slug, None, None).await;
        assert!(page.messages.is_empty(), "mensagem apagada ainda aparece");
    }

    #[tokio::test]
    #[ignore]
    async fn nao_lido_conta_mensagem_dos_outros_e_zera_ao_marcar() {
        let state = test_state().await;
        let (dono, server_slug, texto, _v) = setup(&state).await;

        let membro = make_user(&state).await;
        let _ = join_server(
            membro.clone(),
            State(state.clone()),
            Path(server_slug.clone()),
        )
        .await
        .unwrap();

        send(&state, &dono, &texto.slug, "oi 1").await;
        send(&state, &dono, &texto.slug, "oi 2").await;
        // A própria mensagem do membro não pode contar como não lida pra ele.
        send(&state, &membro, &texto.slug, "respondi").await;

        let detalhe = get_server(
            membro.clone(),
            State(state.clone()),
            Path(server_slug.clone()),
        )
        .await
        .unwrap()
        .0;
        let ch = detalhe
            .channels
            .iter()
            .find(|c| c.slug == texto.slug)
            .unwrap();
        assert_eq!(ch.unread, 2, "só as duas do dono contam");

        let st = mark_read(
            membro.clone(),
            State(state.clone()),
            Path(texto.slug.clone()),
            Json(MarkReadReq { message_id: None }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(st.unread, 0);

        let detalhe = get_server(membro, State(state.clone()), Path(server_slug))
            .await
            .unwrap()
            .0;
        let ch = detalhe
            .channels
            .iter()
            .find(|c| c.slug == texto.slug)
            .unwrap();
        assert_eq!(ch.unread, 0, "depois de marcar lido, zera");
    }

    #[tokio::test]
    #[ignore]
    async fn marcador_de_leitura_nao_anda_pra_tras() {
        let state = test_state().await;
        let (dono, server_slug, texto, _v) = setup(&state).await;

        let membro = make_user(&state).await;
        let _ = join_server(
            membro.clone(),
            State(state.clone()),
            Path(server_slug.clone()),
        )
        .await
        .unwrap();

        let antiga = send(&state, &dono, &texto.slug, "antiga").await;
        send(&state, &dono, &texto.slug, "nova").await;

        // Lê tudo…
        let st = mark_read(
            membro.clone(),
            State(state.clone()),
            Path(texto.slug.clone()),
            Json(MarkReadReq { message_id: None }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(st.unread, 0);

        // …e aí chega, fora de ordem, um "marquei lido até a antiga".
        let st = mark_read(
            membro,
            State(state.clone()),
            Path(texto.slug),
            Json(MarkReadReq {
                message_id: Some(antiga.id),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(
            st.unread, 0,
            "marcador andou pra trás e ressuscitou não-lido que o usuário já viu"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn mensagem_vazia_ou_gigante_e_recusada() {
        let state = test_state().await;
        let (dono, _s, texto, _v) = setup(&state).await;

        for body in ["", "   ", "\n\t "] {
            let err = send_message(
                dono.clone(),
                State(state.clone()),
                Path(texto.slug.clone()),
                Json(SendMessageReq { body: body.into() }),
            )
            .await
            .expect_err("mensagem vazia não passa");
            assert!(matches!(err, AppError::Validation(_)), "veio: {err:?}");
        }

        let gigante = "a".repeat(MAX_BODY + 1);
        let err = send_message(
            dono,
            State(state.clone()),
            Path(texto.slug),
            Json(SendMessageReq { body: gigante }),
        )
        .await
        .expect_err("mensagem acima do limite não passa");
        assert!(matches!(err, AppError::Validation(_)), "veio: {err:?}");
    }
}

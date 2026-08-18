//! Servidores e canais — o modelo que substitui as salas avulsas.
//!
//! Um servidor agrupa canais de texto e de voz. Todo canal tem slug próprio e
//! global: é ele que vira o link compartilhável (`/c/<slug>`), pra que um convite
//! pra canal de voz resolva sozinho, sem exigir o slug do servidor junto.
//!
//! Autorização, em uma frase: ler exige ser membro, criar canal exige ser dono,
//! e entrar exige apenas ter o link.

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::slug::random_slug;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ServerSummary {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub owner_id: Uuid,
    pub created_at: DateTime<Utc>,
    /// Papel de quem pediu — o front usa pra decidir o que mostrar.
    pub role: String,
    pub member_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelDto {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub kind: String,
    pub position: f64,
    /// Quantas pessoas estão no canal de voz agora. Sempre 0 pra canal de texto.
    pub live: usize,
    /// Mensagens não lidas por quem pediu. Sempre 0 pra canal de voz.
    pub unread: i64,
}

#[derive(Debug, Serialize)]
pub struct MemberDto {
    pub user_id: Uuid,
    pub display_name: Option<String>,
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct ServerDetail {
    #[serde(flatten)]
    pub server: ServerSummary,
    pub channels: Vec<ChannelDto>,
    pub members: Vec<MemberDto>,
}

#[derive(Debug, Serialize)]
pub struct ChannelResolved {
    #[serde(flatten)]
    pub channel: ChannelDto,
    pub server_slug: String,
    pub server_name: String,
    /// Se quem pediu já é membro do servidor dono do canal.
    pub is_member: bool,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateServerReq {
    #[validate(length(min = 2, max = 64))]
    pub name: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChannelReq {
    #[validate(length(min = 1, max = 32))]
    pub name: String,
    #[validate(custom(function = "validate_kind"))]
    pub kind: String,
}

fn validate_kind(v: &str) -> Result<(), validator::ValidationError> {
    match v {
        "text" | "voice" => Ok(()),
        _ => Err(validator::ValidationError::new("kind")),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Papel do usuário no servidor, ou `None` se não for membro.
async fn role_in_server(
    state: &AppState,
    server_id: Uuid,
    user_id: Uuid,
) -> AppResult<Option<String>> {
    let row = sqlx::query_scalar!(
        "SELECT role FROM server_members WHERE server_id = $1 AND user_id = $2",
        server_id,
        user_id
    )
    .fetch_optional(&state.db)
    .await?;
    Ok(row)
}

/// Exige que o usuário seja membro. Responde 404 (não 403) quando não é, pra não
/// confirmar a existência de um servidor pra quem não faz parte dele.
async fn require_member(state: &AppState, server_id: Uuid, user_id: Uuid) -> AppResult<String> {
    role_in_server(state, server_id, user_id)
        .await?
        .ok_or(AppError::NotFound)
}

/// Canal de voz reporta presença ao vivo; canal de texto sempre 0.
fn live_count(state: &AppState, kind: &str, slug: &str) -> usize {
    if kind == "voice" {
        state.hub.slug_count(slug)
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Cria o servidor, coloca o autor como dono e abre os dois canais padrão.
///
/// Tudo numa transação: um servidor sem canal nenhum é um estado que o front não
/// sabe renderizar, então ou nasce inteiro ou não nasce. O retry recomeça a
/// transação porque no Postgres um erro aborta a transação inteira.
pub async fn create_server(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateServerReq>,
) -> AppResult<Json<ServerDetail>> {
    body.validate()?;
    let name = body.name.trim().to_string();

    let mut last_err = None;
    for _ in 0..5 {
        let server_slug = random_slug();
        let mut tx = state.db.begin().await?;

        let server = match sqlx::query!(
            r#"
            INSERT INTO servers (slug, name, owner_id)
            VALUES ($1, $2, $3)
            RETURNING id, slug, name, owner_id, created_at
            "#,
            server_slug,
            name,
            auth.id,
        )
        .fetch_one(&mut *tx)
        .await
        {
            Ok(r) => r,
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => continue,
            Err(e) => {
                last_err = Some(e);
                break;
            }
        };

        sqlx::query!(
            "INSERT INTO server_members (server_id, user_id, role) VALUES ($1, $2, 'owner')",
            server.id,
            auth.id,
        )
        .execute(&mut *tx)
        .await?;

        // Canais padrão: um de cada tipo, pro servidor nascer utilizável.
        let mut channels = Vec::new();
        for (name, kind, position) in [("geral", "text", 0.0_f64), ("Geral", "voice", 1.0_f64)] {
            let ch = sqlx::query!(
                r#"
                INSERT INTO channels (server_id, slug, name, kind, position)
                VALUES ($1, $2, $3, $4, $5)
                RETURNING id, slug, name, kind, position
                "#,
                server.id,
                random_slug(),
                name,
                kind,
                position,
            )
            .fetch_one(&mut *tx)
            .await?;
            channels.push(ChannelDto {
                id: ch.id,
                slug: ch.slug,
                name: ch.name,
                kind: ch.kind,
                position: ch.position,
                live: 0,
                unread: 0,
            });
        }

        tx.commit().await?;

        return Ok(Json(ServerDetail {
            server: ServerSummary {
                id: server.id,
                slug: server.slug,
                name: server.name,
                owner_id: server.owner_id,
                created_at: server.created_at,
                role: "owner".into(),
                member_count: 1,
            },
            channels,
            members: vec![MemberDto {
                user_id: auth.id,
                display_name: None,
                role: "owner".into(),
            }],
        }));
    }

    Err(last_err.map(AppError::from).unwrap_or(AppError::Internal(
        "could not generate unique server slug".into(),
    )))
}

/// Servidores dos quais o usuário é membro, mais recentes primeiro.
pub async fn list_servers(
    auth: AuthUser,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<ServerSummary>>> {
    let rows = sqlx::query!(
        r#"
        SELECT s.id, s.slug, s.name, s.owner_id, s.created_at,
               m.role,
               (SELECT COUNT(*) FROM server_members WHERE server_id = s.id) AS "member_count!"
        FROM servers s
        JOIN server_members m ON m.server_id = s.id
        WHERE m.user_id = $1
        ORDER BY s.created_at DESC
        "#,
        auth.id
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| ServerSummary {
                id: r.id,
                slug: r.slug,
                name: r.name,
                owner_id: r.owner_id,
                created_at: r.created_at,
                role: r.role,
                member_count: r.member_count,
            })
            .collect(),
    ))
}

/// Servidor + canais + membros. Exige ser membro.
pub async fn get_server(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> AppResult<Json<ServerDetail>> {
    let server = sqlx::query!(
        "SELECT id, slug, name, owner_id, created_at FROM servers WHERE slug = $1",
        slug
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let role = require_member(&state, server.id, auth.id).await?;

    // O não-lido sai junto do detalhe do servidor porque é o que a sidebar
    // precisa pra marcar o canal — pedir canal a canal seria N requisições.
    // Mensagem própria nunca conta como não lida.
    let channels = sqlx::query!(
        r#"
        SELECT c.id, c.slug, c.name, c.kind, c.position,
               (SELECT COUNT(*) FROM messages m
                 WHERE m.channel_id = c.id
                   AND m.user_id <> $2
                   AND (r.last_read_at IS NULL OR m.created_at > r.last_read_at)
               ) AS "unread!"
        FROM channels c
        LEFT JOIN message_reads r ON r.channel_id = c.id AND r.user_id = $2
        WHERE c.server_id = $1
        ORDER BY c.kind, c.position, c.created_at
        "#,
        server.id,
        auth.id
    )
    .fetch_all(&state.db)
    .await?;

    let members = sqlx::query!(
        r#"
        SELECT m.user_id, m.role, u.display_name
        FROM server_members m
        JOIN users u ON u.id = m.user_id
        WHERE m.server_id = $1
        ORDER BY m.joined_at
        "#,
        server.id
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(ServerDetail {
        server: ServerSummary {
            id: server.id,
            slug: server.slug,
            name: server.name,
            owner_id: server.owner_id,
            created_at: server.created_at,
            role,
            member_count: members.len() as i64,
        },
        channels: channels
            .into_iter()
            .map(|c| ChannelDto {
                live: live_count(&state, &c.kind, &c.slug),
                unread: c.unread,
                id: c.id,
                slug: c.slug,
                name: c.name,
                kind: c.kind,
                position: c.position,
            })
            .collect(),
        members: members
            .into_iter()
            .map(|m| MemberDto {
                user_id: m.user_id,
                display_name: m.display_name,
                role: m.role,
            })
            .collect(),
    }))
}

/// Entra no servidor via link de convite. Idempotente — entrar duas vezes não
/// rebaixa o dono a membro nem duplica a linha.
pub async fn join_server(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> AppResult<Json<ServerSummary>> {
    let server = sqlx::query!(
        "SELECT id, slug, name, owner_id, created_at FROM servers WHERE slug = $1",
        slug
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    sqlx::query!(
        "INSERT INTO server_members (server_id, user_id, role) VALUES ($1, $2, 'member')
         ON CONFLICT (server_id, user_id) DO NOTHING",
        server.id,
        auth.id,
    )
    .execute(&state.db)
    .await?;

    let role = require_member(&state, server.id, auth.id).await?;
    let member_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "c!" FROM server_members WHERE server_id = $1"#,
        server.id
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(ServerSummary {
        id: server.id,
        slug: server.slug,
        name: server.name,
        owner_id: server.owner_id,
        created_at: server.created_at,
        role,
        member_count,
    }))
}

/// Cria canal no servidor. Só o dono.
pub async fn create_channel(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(body): Json<CreateChannelReq>,
) -> AppResult<Json<ChannelDto>> {
    body.validate()?;
    let name = body.name.trim().to_string();

    let server = sqlx::query!("SELECT id FROM servers WHERE slug = $1", slug)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let role = require_member(&state, server.id, auth.id).await?;
    if role != "owner" {
        return Err(AppError::Forbidden("só o dono cria canal".into()));
    }

    // Novo canal vai pro fim da lista do seu tipo.
    let next_position = sqlx::query_scalar!(
        r#"SELECT COALESCE(MAX(position), -1) + 1 AS "p!"
           FROM channels WHERE server_id = $1 AND kind = $2"#,
        server.id,
        body.kind,
    )
    .fetch_one(&state.db)
    .await?;

    let mut last_err = None;
    for _ in 0..5 {
        match sqlx::query!(
            r#"
            INSERT INTO channels (server_id, slug, name, kind, position)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, slug, name, kind, position
            "#,
            server.id,
            random_slug(),
            name,
            body.kind,
            next_position,
        )
        .fetch_one(&state.db)
        .await
        {
            Ok(c) => {
                return Ok(Json(ChannelDto {
                    id: c.id,
                    slug: c.slug,
                    name: c.name,
                    kind: c.kind,
                    position: c.position,
                    live: 0,
                    unread: 0,
                }))
            }
            // Pode ser colisão de slug (retry resolve) ou nome repetido no mesmo
            // tipo de canal (retry não resolve — é erro do usuário).
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
                if db
                    .constraint()
                    .is_some_and(|c| c.contains("channels_server_kind_name_idx"))
                {
                    return Err(AppError::Conflict(
                        "já existe um canal com esse nome".into(),
                    ));
                }
                continue;
            }
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }

    Err(last_err.map(AppError::from).unwrap_or(AppError::Internal(
        "could not generate unique channel slug".into(),
    )))
}

/// Resolve canal pelo slug do link. É o que a tela de convite chama antes de
/// entrar: devolve o canal e diz se quem pediu já é membro, pra decidir entre
/// "entrar direto" e "aceitar convite".
pub async fn get_channel(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> AppResult<Json<ChannelResolved>> {
    let row = sqlx::query!(
        r#"
        SELECT c.id, c.slug, c.name, c.kind, c.position,
               s.id AS server_id, s.slug AS server_slug, s.name AS server_name
        FROM channels c
        JOIN servers s ON s.id = c.server_id
        WHERE c.slug = $1
        "#,
        slug
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let is_member = role_in_server(&state, row.server_id, auth.id)
        .await?
        .is_some();

    Ok(Json(ChannelResolved {
        channel: ChannelDto {
            live: live_count(&state, &row.kind, &row.slug),
            unread: 0,
            id: row.id,
            slug: row.slug,
            name: row.name,
            kind: row.kind,
            position: row.position,
        },
        server_slug: row.server_slug,
        server_name: row.server_name,
        is_member,
    }))
}

// ---------------------------------------------------------------------------
// Testes de integração.
//
// Marcados `#[ignore]` porque precisam de Postgres: `cargo test` sem banco
// continua verde, e o job `integration` do CI roda com `--ignored`. Rodar local:
//
//   DATABASE_URL=postgres://boracall:boracall@127.0.0.1:5433/boracall \
//     cargo test -p boracall-server -- --ignored
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::email::Mailer;
    use crate::otp::OtpStore;
    use crate::signaling::Hub;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;

    pub(crate) async fn test_state() -> AppState {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL pros testes de integração");
        let db = PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .expect("conectar no Postgres de teste");
        sqlx::migrate!("./migrations")
            .run(&db)
            .await
            .expect("migrations");
        AppState {
            db,
            hub: Arc::new(Hub::new()),
            jwt_secret: Arc::new("test-secret-com-pelo-menos-24-chars".into()),
            jwt_ttl_days: 30,
            otp: OtpStore::new(),
            mailer: Mailer::new(None, None),
            max_peers_per_room: 6,
        }
    }

    /// Usuário novo a cada chamada — isola os testes sem precisar truncar tabela.
    pub(crate) async fn make_user(state: &AppState) -> AuthUser {
        let email = format!("{}@teste.local", Uuid::new_v4());
        let id = sqlx::query_scalar!(
            "INSERT INTO users (email, password_hash) VALUES ($1, 'x') RETURNING id",
            email
        )
        .fetch_one(&state.db)
        .await
        .expect("criar usuário");
        AuthUser { id, email }
    }

    fn req(name: &str) -> CreateServerReq {
        CreateServerReq { name: name.into() }
    }

    #[tokio::test]
    #[ignore]
    async fn servidor_novo_nasce_com_dono_e_dois_canais_padrao() {
        let state = test_state().await;
        let dono = make_user(&state).await;

        let out = create_server(dono.clone(), State(state.clone()), Json(req("Time Athmos")))
            .await
            .unwrap()
            .0;

        assert_eq!(out.server.name, "Time Athmos");
        assert_eq!(out.server.role, "owner");
        assert_eq!(out.server.owner_id, dono.id);
        assert_eq!(out.server.member_count, 1);

        // Um canal de cada tipo, pro servidor nascer utilizável.
        assert_eq!(out.channels.len(), 2);
        let kinds: Vec<&str> = out.channels.iter().map(|c| c.kind.as_str()).collect();
        assert!(kinds.contains(&"text"), "faltou canal de texto");
        assert!(kinds.contains(&"voice"), "faltou canal de voz");

        // Todo canal precisa de slug próprio — é o link compartilhável.
        assert_ne!(out.channels[0].slug, out.channels[1].slug);
        for c in &out.channels {
            assert_eq!(c.slug.len(), 5);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn quem_nao_e_membro_nao_enxerga_o_servidor() {
        let state = test_state().await;
        let dono = make_user(&state).await;
        let estranho = make_user(&state).await;

        let s = create_server(dono, State(state.clone()), Json(req("Privado")))
            .await
            .unwrap()
            .0;

        let err = get_server(estranho, State(state.clone()), Path(s.server.slug))
            .await
            .expect_err("estranho não pode ler servidor");

        // 404 e não 403: responder "proibido" confirmaria que o servidor existe.
        assert!(matches!(err, AppError::NotFound), "veio: {err:?}");
    }

    #[tokio::test]
    #[ignore]
    async fn membro_comum_nao_cria_canal() {
        let state = test_state().await;
        let dono = make_user(&state).await;
        let membro = make_user(&state).await;

        let s = create_server(dono, State(state.clone()), Json(req("Aberto")))
            .await
            .unwrap()
            .0;
        let slug = s.server.slug.clone();

        let _ = join_server(membro.clone(), State(state.clone()), Path(slug.clone()))
            .await
            .unwrap();

        let err = create_channel(
            membro,
            State(state.clone()),
            Path(slug),
            Json(CreateChannelReq {
                name: "off-topic".into(),
                kind: "text".into(),
            }),
        )
        .await
        .expect_err("membro comum não pode criar canal");

        assert!(matches!(err, AppError::Forbidden(_)), "veio: {err:?}");
    }

    #[tokio::test]
    #[ignore]
    async fn entrar_duas_vezes_nao_duplica_nem_rebaixa_o_dono() {
        let state = test_state().await;
        let dono = make_user(&state).await;

        let s = create_server(dono.clone(), State(state.clone()), Json(req("Idempotente")))
            .await
            .unwrap()
            .0;
        let slug = s.server.slug.clone();

        let membro = make_user(&state).await;
        let _ = join_server(membro.clone(), State(state.clone()), Path(slug.clone()))
            .await
            .unwrap();
        let segunda = join_server(membro, State(state.clone()), Path(slug.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(segunda.member_count, 2, "entrar de novo não pode duplicar");

        // O dono entrando pelo próprio link continua dono.
        let dono_again = join_server(dono, State(state.clone()), Path(slug))
            .await
            .unwrap()
            .0;
        assert_eq!(dono_again.role, "owner", "dono não pode virar member");
        assert_eq!(dono_again.member_count, 2);
    }

    #[tokio::test]
    #[ignore]
    async fn nome_repetido_colide_so_dentro_do_mesmo_tipo() {
        let state = test_state().await;
        let dono = make_user(&state).await;

        let s = create_server(dono.clone(), State(state.clone()), Json(req("Nomes")))
            .await
            .unwrap()
            .0;
        let slug = s.server.slug.clone();

        let mk = |kind: &str, name: &str| {
            create_channel(
                dono.clone(),
                State(state.clone()),
                Path(slug.clone()),
                Json(CreateChannelReq {
                    name: name.into(),
                    kind: kind.into(),
                }),
            )
        };

        let _ = mk("text", "deploys").await.unwrap();

        // Mesmo nome, mesmo tipo → conflito explicado, não 500.
        let err = mk("text", "deploys").await.expect_err("nome repetido");
        assert!(matches!(err, AppError::Conflict(_)), "veio: {err:?}");

        // Mesmo nome em tipo diferente convive (# deploys e 🔊 deploys).
        let _ = mk("voice", "deploys").await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn link_de_canal_resolve_e_diz_se_ja_e_membro() {
        let state = test_state().await;
        let dono = make_user(&state).await;
        let convidado = make_user(&state).await;

        let s = create_server(dono.clone(), State(state.clone()), Json(req("Convite")))
            .await
            .unwrap()
            .0;
        let voz = s
            .channels
            .iter()
            .find(|c| c.kind == "voice")
            .expect("canal de voz")
            .slug
            .clone();

        // Quem recebeu o link ainda não é membro — a tela de convite usa isso
        // pra decidir entre "entrar direto" e "aceitar convite".
        let resolved = get_channel(convidado.clone(), State(state.clone()), Path(voz.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(resolved.server_name, "Convite");
        assert_eq!(resolved.channel.kind, "voice");
        assert!(!resolved.is_member);

        let _ = join_server(
            convidado.clone(),
            State(state.clone()),
            Path(resolved.server_slug),
        )
        .await
        .unwrap();

        let depois = get_channel(convidado, State(state.clone()), Path(voz))
            .await
            .unwrap()
            .0;
        assert!(depois.is_member, "depois de entrar, é membro");
    }
}

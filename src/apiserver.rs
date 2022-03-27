use std::{path::PathBuf, sync::Arc};

use crate::models::{self, User};
use sqlx::{postgres::PgPoolOptions, PgPool};

use axum::{
    async_trait,
    body::{Bytes, StreamBody},
    extract::{ContentLengthLimit, Extension, FromRequest, Path, RequestParts},
    http::{header, StatusCode},
    response::{Headers, IntoResponse, Json},
    routing::{delete, get, put},
    Router,
};
use sha2::{Digest, Sha256};
use tracing_subscriber::{fmt::MakeWriter, registry::SpanData};

use crate::{apiresponse::ApiError, registry, settings};
use serde_json::{json, Value};
use tracing::info;

use serde_derive::Deserialize;
use serde_derive::Serialize;
use tokio_util::io::ReaderStream;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub errors: Vec<Error>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Error {
    pub detail: String,
}


#[async_trait]
impl<B> FromRequest<B> for models::UserSession
where
    B: Send,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let pool: Option<&PgPool> = req.extensions().unwrap().get();
        let pool = pool.unwrap();

        let mut trans = pool
            .begin()
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?;

        let authorization = req
            .headers()
            .and_then(|headers| headers.get(header::AUTHORIZATION));

        if let Some(authorization) = authorization {
            if let Ok(authorization) = authorization.to_str() {
                if let Ok(Some(session)) =
                    models::UserSession::by_token(&mut trans, authorization).await
                {
                    return Ok(session);
                } else {
                    return Err((StatusCode::FORBIDDEN, "session does not exist"));
                }
            }
        }

        return Err((StatusCode::FORBIDDEN, "`authorization` header is missing"));
    }
}

async fn publish(
    ContentLengthLimit(bytes): ContentLengthLimit<Bytes, { 1024 * 20_000 }>,
    sender: Extension<registry::SyncSender>,
    settings: Extension<Arc<settings::Settings>>,
    pool: Extension<PgPool>,
    session: models::UserSession,
) -> Result<Json<Value>, ApiError> {
    info!("{}", (*settings).repo_path);
    info!("{}", (*settings).storage_path);

    let mut trans = pool.begin().await?;

    // TODO: validate package name
    // todo: handle bad data

    if bytes.len() < 8 {
        return Err(ApiError(
            String::from("Invalid publish request"),
            StatusCode::OK,
        ));
    }

    let json_len = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if json_len as usize > bytes.len() - 8 {
        return Err(ApiError(
            String::from("Invalid publish request"),
            StatusCode::OK,
        ));
    }

    let crate_json: registry::PublishPackage =
        match serde_json::from_slice(&bytes[4..4 + json_len as usize]) {
            Ok(crate_json) => crate_json,
            Err(_) => return Err(ApiError(String::from("Invalid crate json"), StatusCode::OK)),
        };

    let crate_len = u32::from_le_bytes(
        bytes[4 + json_len as usize..8 + json_len as usize]
            .try_into()
            .unwrap(),
    );

    if json_len as usize + crate_len as usize + 8 < bytes.len() {
        return Err(ApiError(
            String::from("Invalid publish request"),
            StatusCode::OK,
        ));
    }

    let crate_data: Vec<u8> =
        bytes[8 + json_len as usize..8 + json_len as usize + crate_len as usize].to_vec();

    let mut hash = Sha256::new();
    hash.update(&crate_data);
    let hash = hash.finalize();
    let hash = hex::encode(hash);

    if models::Crate::exists_by_ident(&mut trans, &crate_json.name).await? {
        if !models::CrateOwner::exists(&mut trans, &crate_json.name, &session.ident).await? {
            return Err(ApiError(
                String::from("User is not a crate owner"),
                StatusCode::OK,
            ));
        }
    } else {
        models::Crate::new(&mut trans, &crate_json.name).await?;
        models::CrateOwner::new(&mut trans, &crate_json.name, &session.ident).await?;
    }

    trans.commit().await?;

    match registry::run_task(
        registry::Operation::Publish(registry::Package::from_pub(crate_json, hash), crate_data),
        sender,
    )
    .await
    .unwrap()
    {
        registry::RegistryResponse::Publish(res) => res.unwrap(),
        _ => unreachable!("o no"),
    };

    Ok(Json(
        json!({ "warnings": {"invalid_categories": [], "invalid_badges": [],"other": []} }),
    ))
}

async fn yank(
    Path((crate_name, version)): Path<(String, String)>,
    sender: Extension<registry::SyncSender>,
    pool: Extension<PgPool>,
    session: models::UserSession,
) -> Result<Json<Value>, ApiError> {
    let mut trans = pool.begin().await?;
    if models::CrateOwner::exists(&mut trans, &crate_name, &session.ident).await? {
        match registry::run_task(registry::Operation::Yank(crate_name, version, true), sender)
            .await
            .unwrap()
        {
            registry::RegistryResponse::Yank(res) => match res {
                Ok(_) => Ok(Json(json!({"ok": true}))),
                Err(registry::YankError::CrateNotFound) => Err(ApiError(
                    String::from("crate should exist but doesnt?"),
                    StatusCode::OK,
                )),
            },
            _ => unreachable!("o no"),
        }
    } else {
        Err(ApiError(
            String::from("crate does not exist!"),
            StatusCode::OK,
        ))
    }
}

async fn unyank(
    Path((crate_name, version)): Path<(String, String)>,
    sender: Extension<registry::SyncSender>,
    pool: Extension<PgPool>,
    session: models::UserSession,
) -> Result<Json<Value>, ApiError> {
    let mut trans = pool.begin().await?;
    if models::CrateOwner::exists(&mut trans, &crate_name, &session.ident).await? {
        match registry::run_task(
            registry::Operation::Yank(crate_name, version, false),
            sender,
        )
        .await
        .unwrap()
        {
            registry::RegistryResponse::Yank(res) => match res {
                Ok(_) => Ok(Json(json!({"ok": true}))),
                Err(registry::YankError::CrateNotFound) => Err(ApiError(
                    String::from("crate should exist but doesnt?"),
                    StatusCode::OK,
                )),
            },
            _ => unreachable!("o no"),
        }
    } else {
        Err(ApiError(
            String::from("crate does not exist!"),
            StatusCode::OK,
        ))
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnersJson {
    pub users: Vec<UserJson>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserJson {
    pub id: u32,
    pub login: String,
    pub name: Option<String>,
}

async fn owners(
    Path(crate_name): Path<String>,
    pool: Extension<PgPool>,
    _session: models::UserSession,
) -> Result<Json<OwnersJson>, ApiError> {
    let mut trans = pool.begin().await?;
    let owners = models::CrateOwner::all_owners(&mut trans, &crate_name).await?;
    let owners_json = OwnersJson {
        users: owners
            .iter()
            .enumerate()
            .map(|(i, x)| UserJson {
                id: i as u32,
                login: x.user_ident.clone(),
                name: None,
            })
            .collect(),
    };

    Ok(Json(owners_json))
}

#[derive(Deserialize)]
pub struct OwnerList {
    users: Vec<String>,
}

async fn add_owners(
    Path(crate_name): Path<String>,
    pool: Extension<PgPool>,
    session: models::UserSession,
    axum::extract::Json(to_add): axum::extract::Json<OwnerList>,
) -> Result<Json<Value>, ApiError> {
    let mut trans = pool.begin().await?;
    if !models::CrateOwner::exists(&mut trans, &crate_name, &session.ident).await? {
        return Err(ApiError(
            String::from("You do not own this crate"),
            StatusCode::OK,
        ));
    }

    if to_add.users.len() > 255 {
        return Err(ApiError(
            String::from("You can only add up to 255 owners at once"),
            StatusCode::OK,
        ));
    }

    for owner in to_add.users {
        if !models::User::exists_by_ident(&mut trans, &owner).await? {
            return Err(ApiError(
                format!("The user {} does not exist", owner),
                StatusCode::OK,
            ));
        }

        if !models::CrateOwner::exists(&mut trans, &crate_name, &owner).await? {
            models::CrateOwner::new(&mut trans, &crate_name, &owner).await?;
        }
    }
    trans.commit().await?;

    Ok(Json(
        json!({"ok": true, "msg": "added owners successfully"}),
    ))
}

async fn remove_owners(
    Path(crate_name): Path<String>,
    pool: Extension<PgPool>,
    session: models::UserSession,
    axum::extract::Json(to_delete): axum::extract::Json<OwnerList>,
) -> Result<Json<Value>, ApiError> {
    let mut trans = pool.begin().await?;
    if !models::CrateOwner::exists(&mut trans, &crate_name, &session.ident).await? {
        return Err(ApiError(
            String::from("You do not own this crate"),
            StatusCode::OK,
        ));
    }

    if to_delete.users.len() > 255 {
        return Err(ApiError(
            String::from("You can only delete up to 255 owners at once"),
            StatusCode::OK,
        ));
    }

    for owner in to_delete.users {
        if !models::User::exists_by_ident(&mut trans, &owner).await? {
            return Err(ApiError(
                format!("The user {} does not exist", owner),
                StatusCode::OK,
            ));
        }

        if models::CrateOwner::exists(&mut trans, &crate_name, &owner).await? {
            models::CrateOwner::delete(&mut trans, &crate_name, &owner).await?;
        }
    }
    trans.commit().await?;

    Ok(Json(
        json!({"ok": true, "msg": "deleted owners successfully"}),
    ))
}

async fn dl(Path(hash): Path<String>, settings: Extension<Arc<settings::Settings>>) -> impl IntoResponse {
    let mut file_path = PathBuf::from(&settings.storage_path);
    if hash.len() != 64 || hash.contains('.') || hash.contains('/') {
        return Err((StatusCode::NOT_FOUND, "File not found!"));
    }

    file_path.push(hash);
    file_path.set_extension("crate");

    let file = match tokio::fs::File::open(file_path).await {
        Ok(file) => file,
        Err(_) => return Err((StatusCode::NOT_FOUND, "File not found!")),
    };

    let stream = ReaderStream::new(file);
    let body = StreamBody::new(stream);

    let headers = Headers([
        (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
        (header::CONTENT_DISPOSITION, "attachment; filename=\"\""),
    ]);

    Ok((headers, body))
}

fn build_router(
    sender: registry::SyncSender,
    settings: Arc<settings::Settings>,
    pool: PgPool,
) -> Router {
    Router::new()
        .route("/me", get(|| async { "uwu" }))
        .route("/api/v1/crates/new", put(publish))
        .route("/api/v1/crates/:crate_name/:version/yank", delete(yank))
        .route("/api/v1/crates/:crate_name/:version/unyank", put(unyank))
        .route("/api/v1/dl/:hash", get(dl))
        .route(
            "/api/v1/crates/:crate_name/owners",
            get(owners).put(add_owners).delete(remove_owners),
        )
        .layer(axum::extract::Extension(sender))
        .layer(axum::extract::Extension(settings))
        .layer(axum::extract::Extension(pool))
}

#[derive(Debug, thiserror::Error)]
pub enum ApiServerError {
    #[error("Sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("Failed to run database migrations: {0}")]
    SqlxMigration(#[from] sqlx::migrate::MigrateError),
    #[error("Hyper error: {0}")]
    Hyper(#[from] hyper::Error),
}

pub async fn serve(
    sender: registry::SyncSender,
    settings: settings::Settings,
    pool: PgPool,
) -> Result<(), ApiServerError> {
    axum::Server::bind(&"0.0.0.0:8080".parse().unwrap())
        .serve(
            build_router(
                sender,
                Arc::new(settings),
                pool
            )
            .into_make_service(),
        )
        .await?;

    Ok(())
}

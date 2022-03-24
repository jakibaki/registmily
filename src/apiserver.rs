use std::path::PathBuf;

use crate::models;
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
use tracing_subscriber::registry::SpanData;

use crate::{registry, settings};
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

#[derive(Clone)]
struct DataPaths {
    git_path: String,
    storage_path: String,
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
    data_paths: Extension<DataPaths>,
    pool: Extension<PgPool>,
    session: models::UserSession,
) -> Json<Value> {
    info!("{}", (*data_paths).git_path);
    info!("{}", (*data_paths).storage_path);

    let invalid_publish_err = Json(json!({"errors": [{"detail": "publish request corrupted"}]}));

    let mut trans = pool.begin().await.unwrap();

    // TODO: handle errors differently, so much clutter

    // TODO: validate package name
    // todo: handle bad data

    if bytes.len() < 8 {
        return invalid_publish_err;
    }

    let json_len = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if json_len as usize > bytes.len() - 8 {
        return invalid_publish_err;
    }

    let crate_json: registry::PublishPackage =
        match serde_json::from_slice(&bytes[4..4 + json_len as usize]) {
            Ok(crate_json) => crate_json,
            Err(_) => return Json(json!({"errors": [{"detail": "invalid crate json"}]})),
        };

    let crate_len = u32::from_le_bytes(
        bytes[4 + json_len as usize..8 + json_len as usize]
            .try_into()
            .unwrap(),
    );

    if json_len as usize + crate_len as usize + 8 < bytes.len() {
        return invalid_publish_err;
    }

    let crate_data: Vec<u8> =
        bytes[8 + json_len as usize..8 + json_len as usize + crate_len as usize].to_vec();

    let mut hash = Sha256::new();
    hash.update(&crate_data);
    let hash = hash.finalize();
    let hash = hex::encode(hash);

    if let Ok(exists) = models::Crate::exists_by_ident(&mut trans, &crate_json.name).await {
        if exists {
            if let Ok(owned) =
                models::CrateOwner::exists(&mut trans, &crate_json.name, &session.ident).await
            {
                if !owned {
                    return Json(json!({"errors": [{"detail": "user is not a crate owner"}]}));
                }
            } else {
                return Json(json!({"errors": [{"detail": "database error"}]}));
            }
        } else {
            models::Crate::new(&mut trans, &crate_json.name)
                .await
                .unwrap();
            models::CrateOwner::new(&mut trans, &crate_json.name, &session.ident)
                .await
                .unwrap();
        }
    } else {
        return Json(json!({"errors": [{"detail": "database error"}]}));
    }

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

    trans.commit().await.unwrap();
    Json(json!({ "warnings": {"invalid_categories": [], "invalid_badges": [],"other": []} }))
}

async fn yank(
    Path((crate_name, version)): Path<(String, String)>,
    sender: Extension<registry::SyncSender>,
    pool: Extension<PgPool>,
    session: models::UserSession,
) -> Json<Value> {
    let mut trans = pool.begin().await.unwrap();
    if let Ok(true) = models::CrateOwner::exists(&mut trans, &crate_name, &session.ident).await {
        match registry::run_task(registry::Operation::Yank(crate_name, version, true), sender)
            .await
            .unwrap()
        {
            registry::RegistryResponse::Yank(res) => match res {
                Ok(_) => Json(json!({"ok": true})),
                Err(registry::YankError::CrateNotFound) => {
                    Json(json!({"errors": [{"detail": "crate not found!"}]}))
                }
            },
            _ => unreachable!("o no"),
        }
    } else {
        Json(json!({"errors": [{"detail": "You do not own this!"}]}))
    }
}

async fn unyank(
    Path((crate_name, version)): Path<(String, String)>,
    sender: Extension<registry::SyncSender>,
    pool: Extension<PgPool>,
    session: models::UserSession,
) -> Json<Value> {
    let mut trans = pool.begin().await.unwrap();
    if let Ok(true) = models::CrateOwner::exists(&mut trans, &crate_name, &session.ident).await {
        match registry::run_task(registry::Operation::Yank(crate_name, version, false), sender)
            .await
            .unwrap()
        {
            registry::RegistryResponse::Yank(res) => match res {
                Ok(_) => Json(json!({"ok": true})),
                Err(registry::YankError::CrateNotFound) => {
                    Json(json!({"errors": [{"detail": "crate not found!"}]}))
                }
            },
            _ => unreachable!("o no"),
        }
    } else {
        Json(json!({"errors": [{"detail": "You do not own this!"}]}))
    }
}

async fn dl(Path(hash): Path<String>, data_paths: Extension<DataPaths>) -> impl IntoResponse {
    let mut file_path = PathBuf::from(&data_paths.storage_path);
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
    git_path: String,
    storage_path: String,
    pool: PgPool,
) -> Router {
    Router::new()
        .route("/me", get(|| async { "uwu" }))
        .route("/api/v1/crates/new", put(publish))
        .route("/api/v1/crates/:crate_name/:version/yank", delete(yank))
        .route("/api/v1/crates/:crate_name/:version/unyank", put(unyank))
        .route("/api/v1/dl/:hash", get(dl))
        .layer(axum::extract::Extension(sender))
        .layer(axum::extract::Extension(DataPaths {
            git_path,
            storage_path,
        }))
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
                settings.repo_path.clone(),
                settings.storage_path.clone(),
                pool,
            )
            .into_make_service(),
        )
        .await?;

    Ok(())
}

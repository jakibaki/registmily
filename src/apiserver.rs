use std::path::PathBuf;

use axum::{
    body::{Bytes, StreamBody},
    extract::{ContentLengthLimit, Extension, Path},
    http::{header, StatusCode},
    response::{Headers, IntoResponse, Json},
    routing::{delete, get, put},
    Router,
};
use sha2::{Digest, Sha256};

use crate::registry;
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

async fn publish(
    ContentLengthLimit(bytes): ContentLengthLimit<Bytes, { 1024 * 20_000 }>,
    sender: Extension<registry::SyncSender>,
    data_paths: Extension<DataPaths>,
) -> Json<Value> {
    info!("{}", (*data_paths).git_path);
    info!("{}", (*data_paths).storage_path);

    let invalid_publish_err = Json(json!({"errors": [{"detail": "publish request corrupted?"}]}));

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

    Json(json!({ "warnings": {"invalid_categories": [], "invalid_badges": [],"other": []} }))
}

async fn yank(
    Path((crate_name, version)): Path<(String, String)>,
    sender: Extension<registry::SyncSender>,
) -> Json<Value> {
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
}

async fn unyank(
    Path((crate_name, version)): Path<(String, String)>,
    sender: Extension<registry::SyncSender>,
) -> Json<Value> {
    match registry::run_task(
        registry::Operation::Yank(crate_name, version, false),
        sender,
    )
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

fn build_router(sender: registry::SyncSender, git_path: String, storage_path: String) -> Router {
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
}

pub async fn serve(sender: registry::SyncSender, git_path: String, storage_path: String) {
    axum::Server::bind(&"0.0.0.0:8080".parse().unwrap())
        .serve(build_router(sender, git_path, storage_path).into_make_service())
        .await
        .unwrap();
}

use axum::{routing::get, Router};

use crate::registry;

pub struct APIServer {}

impl APIServer {
    pub async fn serve(
        sender: tokio::sync::mpsc::Sender<(
            registry::Operation,
            tokio::sync::oneshot::Sender<registry::RegistryResponse>,
        )>,
    ) {
        axum::Server::bind(&"0.0.0.0:8080".parse().unwrap())
            .serve(
                Router::new()
                    .route(
                        "/",
                        get(|sender| async {
                            let res = registry::add(registry::Operation::Add(12, 13), sender).await.unwrap();
                            if let registry::RegistryResponse::Add(resp) = res {
                                resp
                            } else {
                                ":c".to_string()
                            }
                        }),
                    )
                    .layer(axum::extract::Extension(sender))
                    .into_make_service(),
            )
            .await
            .unwrap();
    }
}

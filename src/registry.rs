use axum::extract;
use git2::Repository;
use std::time::Duration;
use std::{thread, time};
use tokio::sync::RwLock;
use tracing::info;

#[derive(Debug, Clone, Copy)]
pub enum Operation {
    Add(i64, i64),
}

pub enum RegistryResponse {
    Add(String),
}

macro_rules! definition {
    ($name:ident, $operation:ident) => {
        pub async fn $name(
            operation: Operation,
            handler: extract::Extension<
                tokio::sync::mpsc::Sender<(
                    Operation,
                    tokio::sync::oneshot::Sender<RegistryResponse>,
                )>,
            >,
        ) -> Result<RegistryResponse, &'static str> {
            let (sender, recv) = tokio::sync::oneshot::channel();
            if handler.send((operation, sender)).await.is_err() {
                return Err("Sender channel died");
            };

            recv.await.map_err(|_| "Oneshot channel died")
        }
    };
}

pub struct Registry {
    repo: Repository,
}

impl Registry {
    fn new(git_location: &str) -> Self {
        info!("Opening {}", git_location);
        let repo = match Repository::open(git_location) {
            Ok(repo) => repo,
            Err(e) => panic!("failed to open {}", e),
        };

        Self { repo }
    }

    fn publish(&self) {
        unimplemented!()
    }

    fn yank(&self, crate_name: String, version: String) { // TODO: version type
        unimplemented!()
    }

    fn unyank(&self, crate_name: String, version: String) {
        unimplemented!()
    }

    fn list_owners(&self, crate_name: String) {
        unimplemented!()
    }

    fn add_owner(&self, owner: String) {
        unimplemented!()
    }

    fn del_owner(&self, owner: String) {
        unimplemented!()
    }

    fn find_crates(&self, query: String) {
        unimplemented!()
    }
}

definition!(add, Add);

pub fn handler(
    git_location: &str,
    mut recv: tokio::sync::mpsc::Receiver<(
        Operation,
        tokio::sync::oneshot::Sender<RegistryResponse>,
    )>,
) {
    // The git2-rs library is not thread safe and needs to stay on the same thread at all points in time due to it's use of environment variables

    let registry = Registry::new(git_location);

    while let Some((op, oneshot_sender)) = recv.blocking_recv() {
        let _ = oneshot_sender.send(match op {
            Operation::Add(a, b) => {
                thread::sleep(time::Duration::from_millis(2000));
                RegistryResponse::Add((a + b).to_string())
            }
        });
    }
}

mod registry;
mod apiserver;

use tracing::{info, Level};
use tracing_subscriber;
use axum::{extract, routing::get, Router};



#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let (sender, recv) = tokio::sync::mpsc::channel(u16::MAX as usize);
    let jh = std::thread::spawn(move || registry::handler("testgit", recv));

    info!("Starting up");

    apiserver::APIServer::serve(sender).await;

    jh.join().unwrap();

}

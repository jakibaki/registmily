mod apiserver;
mod registry;

use tracing::{info, Level};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();
    let git_path = "testgit";
    let storage_path = "storage";

    let (sender, recv) = tokio::sync::mpsc::channel(u16::MAX as usize);
    let jh = std::thread::spawn(move || registry::handler(git_path, storage_path, recv));

    info!("Starting up");

    apiserver::serve(sender, String::from(git_path), String::from(storage_path)).await;

    jh.join().unwrap();
}

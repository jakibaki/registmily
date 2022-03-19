mod apiserver;
mod registry;
mod settings;
use tracing::{info, Level};



#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let config = match settings::read() {
        Ok(config) => config,
        Err(_) => panic!("could not read config file, check readme for more instructions"),
    };

    let repo_path = config.repo_path.clone();
    let storage_path = config.storage_path.clone();
    


    let (sender, recv) = tokio::sync::mpsc::channel(u16::MAX as usize);
    let jh = std::thread::spawn(move || registry::handler(&config.repo_path, &config.storage_path, recv));

    info!("Starting up");


    apiserver::serve(sender, repo_path, storage_path).await;


    jh.join().unwrap();
}

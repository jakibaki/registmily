mod apiserver;
mod registry;
mod settings;
mod models;
use tracing::{info, Level};



#[tokio::main]
async fn main() -> Result<(), apiserver::ApiServerError> {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let config = match settings::read() {
        Ok(config) => config,
        Err(_) => panic!("could not read config file, check readme for more instructions"),
    };

    let repo_path = config.repo_path.clone();
    let storage_path = config.storage_path.clone();
    


    let (sender, recv) = tokio::sync::mpsc::channel(u16::MAX as usize);
    let jh = std::thread::spawn(move || registry::handler(&repo_path, &storage_path, recv));

    info!("Starting up");


    apiserver::serve(sender, config).await?;


    jh.join().unwrap();

    Ok(())
}

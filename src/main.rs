mod apiresponse;
mod apiserver;
mod models;
mod openid;
mod registry;
mod settings;
use sqlx::postgres::PgPoolOptions;
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

    info!("Connecting to DB");

    let pool = PgPoolOptions::new()
        .max_connections(config.database_connections)
        .connect(&config.database_url)
        .await?;

    info!("Running migrations");
    sqlx::migrate!("./migrations").run(&pool).await?;

    info!("Database setup done, starting api server");

    apiserver::serve(sender, config, pool).await?;

    jh.join().unwrap();

    Ok(())
}

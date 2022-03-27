use config::Config;



#[derive(Debug, Clone, serde::Deserialize)]
pub struct Settings {
    pub repo_path: String,
    pub storage_path: String,
    pub database_url: String,
    pub database_connections: u32
}

pub fn read() -> Result<Settings, config::ConfigError> {
    Config::builder()
        .add_source(config::File::with_name("config"))
        .add_source(config::Environment::with_prefix("REGISTMILY"))
        .build()?
        .try_deserialize()
}

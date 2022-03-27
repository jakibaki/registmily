use config::Config;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Settings {
    pub repo_path: String,
    pub storage_path: String,
    pub database_url: String,
    pub database_connections: u32,
    pub openid_auth_endpoint: String,
    pub openid_token_endpoint: String,
    pub openid_client_id: String,
    pub openid_client_secret: String,
    pub openid_nonce: String,
    pub jwt_key_config: openid_client::config::JwtParsedKeyConfig,
}

pub fn read() -> Result<Settings, config::ConfigError> {
    Config::builder()
        .add_source(config::File::with_name("config"))
        .add_source(config::Environment::with_prefix("REGISTMILY"))
        .build()?
        .try_deserialize()
}

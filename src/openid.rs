use crate::models;
use crate::{apiresponse::ApiError, settings};
use axum::{
    extract::Extension,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use openid_types::token::CodeTokenClaims;
use sqlx::PgPool;
use std::sync::Arc;

use jsonwebtoken::TokenData;
use openid_client::{
    provider::{CallbackResponse, RegisteredProviderBuilder, TokenHandler},
    Client,
};

const PROVIDER_SLUG: &str = "registmily";

#[derive(Debug)]
struct OpenidProvider(PgPool);

#[async_trait::async_trait]
impl TokenHandler for OpenidProvider {
    type Extra = ();
    async fn token_callback(
        &self,
        provider_slug: &str,
        _access_token: &str,
        token_data: TokenData<CodeTokenClaims<Self::Extra>>,
    ) -> CallbackResponse {
        let mut trans = match self.0.begin().await.map_err(ApiError::from) {
            Ok(x) => x,
            Err(why) => {
                return why.into_response();
            }
        };

        let userident = format!("{}-{}", provider_slug, token_data.claims.base.sub);

        match models::User::exists_by_ident(&mut trans, &userident)
            .await
            .map_err(ApiError::from)
        {
            Ok(exists) => {
                if !exists {
                    if let Err(why) = models::User::new(&mut trans, &userident)
                        .await
                        .map_err(ApiError::from)
                    {
                        return why.into_response();
                    }
                }
            }
            Err(why) => return why.into_response(),
        }

        let session = match models::UserSession::new(&mut trans, &userident)
            .await
            .map_err(ApiError::from)
        {
            Ok(x) => x,
            Err(why) => return why.into_response(),
        };

        if let Err(why) = trans.commit().await.map_err(ApiError::from) {
            return why.into_response();
        }

        Response::builder()
            .status(StatusCode::OK)
            .body(axum::body::boxed(axum::body::Body::from(format!(
                "Hello! Your userid is {} and your cargo accesss token is {}",
                session.ident, session.token
            ))))
            .unwrap()
    }
}

pub async fn me(
    settings: Extension<Arc<settings::Settings>>,
    client: Extension<Arc<Client>>,
) -> Response {
    // TODO: use a not so terrible nonce
    match openid_client::axum::build_provider_response::<()>(
        &client.0,
        PROVIDER_SLUG,
        Some(settings.openid_nonce.clone()),
        vec![],
    )
    .await
    {
        Ok(res) => res,
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(axum::body::boxed(axum::body::Body::from(format!(
                "internal error: {:?}",
                err
            ))))
            .unwrap(),
    }
}

pub async fn build_client(settings: Arc<settings::Settings>, pool: PgPool) -> Client {
    Client::builder(settings.jwt_key_config.clone())
        .add_provider(
            RegisteredProviderBuilder::new(
                String::from(PROVIDER_SLUG),
                Some(String::from("http://localhost:8080/callback")),
                settings.openid_client_id.clone(),
                settings.openid_client_secret.clone(),
                Some(url::Url::parse(&settings.openid_auth_endpoint.clone()).unwrap()),
                url::Url::parse(&settings.openid_token_endpoint.clone()).unwrap(),
                Some(OpenidProvider(pool)),
            )
            .finish(),
        )
        .finish()
}

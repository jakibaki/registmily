use axum::response::{IntoResponse, Response};
use hyper::StatusCode;
use serde_derive::Deserialize;
use serde_derive::Serialize;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ApiErrorResponse {
    pub errors: Vec<ApiErrorInfo>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ApiErrorInfo {
    pub detail: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ApiError(pub String, pub StatusCode);

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!("Database error: {}", err);
        Self(
            String::from("Database Error"),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let error_info = ApiErrorInfo { detail: self.0 };
        let error_json = ApiErrorResponse {
            errors: vec![error_info],
        };
        let json = serde_json::to_string(&error_json).unwrap();
        let builder = Response::builder().status(self.1);

        match builder
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::boxed(axum::body::Full::from(json.into_bytes())))
        {
            Ok(res) => res,
            Err(err) => {
                tracing::error!("Failed to build response: {}", err);
                let mut response = Response::default();
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                response
            }
        }
    }
}

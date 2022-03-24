use super::{DbResult, Exists, PgTransaction};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

#[derive(Debug, sqlx::FromRow)]
pub struct User {
    pub ident: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct UserSession {
    pub ident: String,
    pub token: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct Crate {
    pub name: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct CrateOwner {
    pub crate_name: String,
    pub user_ident: String,
}

impl User {
    pub async fn exists_by_ident(
        transaction: &mut PgTransaction<'_>,
        ident: &str,
    ) -> DbResult<bool> {
        Ok(sqlx::query_as!(
            super::Exists,
            "SELECT EXISTS(SELECT 1 FROM users WHERE ident = $1)",
            ident
        )
        .fetch_one(&mut *transaction)
        .await?
        .into())
    }

    pub async fn new(transaction: &mut PgTransaction<'_>, ident: &str) -> DbResult<Self> {
        sqlx::query_as!(
            Self,
            "INSERT INTO users (ident) VALUES ($1) RETURNING ident",
            ident
        )
        .fetch_one(transaction)
        .await
    }

    pub async fn delete(transaction: &mut PgTransaction<'_>, ident: &str) -> DbResult<()> {
        sqlx::query!("DELETE FROM users WHERE ident = $1", ident)
            .execute(transaction)
            .await
            .map(|_| ())
    }
}

impl UserSession {
    pub async fn by_token(
        transaction: &mut PgTransaction<'_>,
        token: &str,
    ) -> DbResult<Option<Self>> {
        sqlx::query_as!(
            Self,
            "SELECT ident, token FROM user_sessions WHERE token = $1",
            token
        )
        .fetch_optional(transaction)
        .await
    }

    pub async fn delete_by_token(transaction: &mut PgTransaction<'_>, token: &str) -> DbResult<()> {
        sqlx::query!("DELETE FROM user_sessions WHERE token = $1", token)
            .execute(transaction)
            .await
            .map(|_| ())
    }

    pub async fn delete_by_ident(transaction: &mut PgTransaction<'_>, ident: &str) -> DbResult<()> {
        sqlx::query!("DELETE FROM user_sessions WHERE ident = $1", ident)
            .execute(transaction)
            .await
            .map(|_| ())
    }

    pub async fn new(transaction: &mut PgTransaction<'_>, ident: &str) -> DbResult<Self> {
        let token: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(60)
            .map(char::from)
            .collect();

        sqlx::query_as!(
            Self,
            "INSERT INTO user_sessions (ident, token) VALUES ($1, $2) RETURNING ident, token",
            ident,
            token
        )
        .fetch_one(transaction)
        .await
    }
}

impl Crate {
    pub async fn exists_by_ident(
        transaction: &mut PgTransaction<'_>,
        name: &str,
    ) -> DbResult<bool> {
        Ok(sqlx::query_as!(
            super::Exists,
            "SELECT EXISTS(SELECT 1 FROM crates WHERE name = $1)",
            name
        )
        .fetch_one(&mut *transaction)
        .await?
        .into())
    }

    pub async fn new(transaction: &mut PgTransaction<'_>, name: &str) -> DbResult<Self> {
        sqlx::query_as!(
            Self,
            "INSERT INTO crates (name) VALUES ($1) RETURNING name",
            name
        )
        .fetch_one(transaction)
        .await
    }

    pub async fn delete(transaction: &mut PgTransaction<'_>, name: &str) -> DbResult<()> {
        sqlx::query!("DELETE FROM crates WHERE name = $1", name)
            .execute(transaction)
            .await
            .map(|_| ())
    }
}

impl CrateOwner {
    pub async fn new(
        transaction: &mut PgTransaction<'_>,
        crate_name: &str,
        user_ident: &str,
    ) -> DbResult<Self> {
        sqlx::query_as!(
			Self,
			"INSERT INTO crate_owners (crate_name, user_ident) VALUES ($1, $2) RETURNING crate_name, user_ident",
			crate_name, user_ident
		)
		.fetch_one(transaction)
		.await
    }

    pub async fn delete(
        transaction: &mut PgTransaction<'_>,
        crate_name: &str,
        user_ident: &str,
    ) -> DbResult<()> {
        sqlx::query!(
            "DELETE FROM crate_owners WHERE crate_name = $1 AND user_ident = $2",
            crate_name,
            user_ident
        )
        .execute(transaction)
        .await
        .map(|_| ())
    }

    pub async fn exists(
        transaction: &mut PgTransaction<'_>,
        crate_name: &str,
        user_ident: &str,
    ) -> DbResult<bool> {
        Ok(sqlx::query_as!(
            super::Exists,
            "SELECT EXISTS(SELECT 1 FROM crate_owners WHERE crate_name = $1 AND user_ident = $2)",
            crate_name,
            user_ident
        )
        .fetch_one(&mut *transaction)
        .await?
        .into())
    }
}

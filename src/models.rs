pub type DbResult<T> = Result<T, sqlx::Error>;
pub type PgTransaction<'c> = sqlx::Transaction<'c, sqlx::Postgres>;


mod registry;

pub use registry::{
	User, UserSession, Crate, CrateOwner
};


#[derive(Debug, sqlx::FromRow)]
struct Exists {
	exists: Option<bool>,
}



impl From<Exists> for bool {
	fn from(e: Exists) -> Self {
		e.exists.unwrap_or_default()
	}
}


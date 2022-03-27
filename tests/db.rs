use registmily::models;

#[sqlx_database_tester::test(pool(variable = "pool"))]
pub async fn db_test() -> Result<(), Box<dyn std::error::Error>> {
    let mut trans = pool.begin().await?;

    assert!(!models::User::exists_by_ident(&mut trans, "peter").await?);
    models::User::new(&mut trans, "peter").await?;
    assert!(models::User::exists_by_ident(&mut trans, "peter").await?);
    let session = models::UserSession::new(&mut trans, "peter").await?;
    let session_from_tok = models::UserSession::by_token(&mut trans, &session.token)
        .await?
        .unwrap();

    assert_eq!(session.ident, session_from_tok.ident);
    assert_eq!(session.token, session_from_tok.token);

    let new_crate = models::Crate::new(&mut trans, "owo").await?;
    models::CrateOwner::new(&mut trans, &new_crate.name, "peter").await?;

    let owners = models::CrateOwner::all_owners(&mut trans, "owo").await?;
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].user_ident, "peter");

    models::Crate::delete(&mut trans, "owo").await?;

    assert!(!models::Crate::exists_by_ident(&mut trans, "owo").await?);

    let owners = models::CrateOwner::all_owners(&mut trans, "owo").await?;
    assert_eq!(owners.len(), 0);

    Ok(())
}

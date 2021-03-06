use openid_client::config::AlgorithmWrappedKeys;
use openid_client::config::JwtKeyConfig;
use openid_client::config::SymmetricKeyConfig;
use registmily::apiserver;
use registmily::init_registry;
use registmily::models;
use registmily::registry;
use registmily::settings;
use serde_json::json;
use std::fs;
use std::path::Path;
use time::Duration;
use tokio::task;
use tracing::{info, Level};

#[test]
pub fn test_registry() {
    assert_eq!(
        registry::get_package_git_path("testgit", "a").as_path(),
        Path::new("testgit/1/a")
    );
    assert_eq!(
        registry::get_package_git_path("testgit", "ab").as_path(),
        Path::new("testgit/2/ab")
    );
    assert_eq!(
        registry::get_package_git_path("testgit", "abc").as_path(),
        Path::new("testgit/3/a/abc")
    );
    assert_eq!(
        registry::get_package_git_path("testgit", "abcd").as_path(),
        Path::new("testgit/ab/cd/abcd")
    );
    assert_eq!(
        registry::get_package_git_path("testgit", "eliseissuperdupercute").as_path(),
        Path::new("testgit/el/is/eliseissuperdupercute")
    );
}

#[sqlx_database_tester::test(pool(variable = "pool"))]
pub async fn e2e_tests() -> Result<(), Box<dyn std::error::Error>> {
    let username = "emily";
    let mut trans = pool.begin().await?;

    models::User::new(&mut trans, username).await?;

    let session = models::UserSession::new(&mut trans, username).await?;

    trans.commit().await?;

    let new_post_json = json!({
        "name": "foo",
        "vers": "0.1.0",
        "deps": [
            {
                "name": "rand",
                "version_req": "^0.6",
                "features": ["i128_support"],
                "optional": false,
                "default_features": true,
                "target": null,
                "kind": "normal",
                "registry": null,
                "explicit_name_in_toml": null,
            }
        ],
        "features": {
            "extras": ["rand/simd_support"]
        },
        "authors": ["Alice <a@example.com>"],
        "description": null,
        "documentation": null,
        "homepage": null,
        "readme": null,
        "readme_file": null,
        "keywords": [],
        "categories": [],
        "license": null,
        "license_file": null,
        "repository": null,
        "badges": {
            "travis-ci": {
                "branch": "master",
                "repository": "rust-lang/cargo"
            }
        },
        "links": null
    });

    let mut expected_index_json = json!({
       "name":"foo",
       "vers":"0.1.0",
       "deps":[
          {
             "name":"rand",
             "req":"^0.6",
             "features":[
                "i128_support"
             ],
             "optional":false,
             "default_features":true,
             "target":null,
             "kind":"normal",
             "registry":null,
             "package":null
          }
       ],
       "cksum":"43cae2eafda4d7a9b31768c8a6f086d7942e97d3a96c75326b3a1f4b17b1cffd",
       "features":{
          "extras":[
             "rand/simd_support"
          ]
       },
       "yanked":false,
       "links":null
    });

    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    // Scary
    let _ = fs::remove_dir_all("e2e_test_repo");
    let _ = fs::remove_dir_all("e2e_test_storage");
    init_registry::setup_registry(
        Path::new("e2e_test_repo"),
        Path::new("e2e_test_storage"),
        "http://localhost:8080",
    );

    let config = settings::Settings {
        repo_path: String::from("e2e_test_repo"),
        storage_path: String::from("e2e_test_storage"),
        database_url: String::from(""),
        database_connections: 0,
        openid_auth_endpoint: String::from("https://gitlab.com/oauth/authorize"),
        openid_token_endpoint: String::from("https://gitlab.com/oauth/token"),
        openid_client_id: String::from(""),
        openid_client_secret: String::from(""),
        openid_nonce: String::from("uwu"),
        jwt_key_config: JwtKeyConfig {
            algorithm_wrapped_key: AlgorithmWrappedKeys::HS512 {
                key: SymmetricKeyConfig {
                    secret: String::from("uwu"),
                },
            },
            expires_in: Duration::seconds(666420),
        }
        .parse_keys()
        .unwrap(),
    };

    let config_repo_path = config.repo_path.clone();
    let config_storage_path = config.storage_path.clone();

    let (sender, recv) = tokio::sync::mpsc::channel(u16::MAX as usize);
    std::thread::spawn(move || registry::handler(&config_repo_path, &config_storage_path, recv));

    info!("Registry handler spawned");

    task::spawn(apiserver::serve(sender, config, pool));
    task::yield_now().await;

    info!("Apiserver spawned");

    // {
    //     let resp = reqwest::get("http://localhost:8080/me").await?;
    //     assert_eq!(resp.text().await?, "uwu");
    // }

    let crate_file = "owo";

    {
        let mut new_body: Vec<u8> = Vec::new();

        let new_json_str = serde_json::to_string(&new_post_json)?;
        let json_len = new_json_str.len() as u32;
        new_body.extend_from_slice(&json_len.to_le_bytes());
        new_body.extend_from_slice(new_json_str.as_bytes());
        let crate_file_len = crate_file.len() as u32;
        new_body.extend_from_slice(&crate_file_len.to_le_bytes());
        new_body.extend_from_slice(crate_file.as_bytes());

        let client = reqwest::Client::new();
        // todo: ensure response fine
        client
            .put("http://localhost:8080/api/v1/crates/new")
            .header("authorization", &session.token)
            .body(new_body)
            .send()
            .await?;

        let real_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string("e2e_test_repo/3/f/foo")?)?;
        assert_eq!(real_json, expected_index_json);
    }

    {
        let crate_file_real = reqwest::get("http://localhost:8080/api/v1/dl/43cae2eafda4d7a9b31768c8a6f086d7942e97d3a96c75326b3a1f4b17b1cffd").await?;
        let crate_file_real = crate_file_real.text().await?;
        assert_eq!(crate_file, crate_file_real);
        assert_eq!(crate_file, &fs::read_to_string("e2e_test_storage/43cae2eafda4d7a9b31768c8a6f086d7942e97d3a96c75326b3a1f4b17b1cffd.crate")?);
    }

    {
        let client = reqwest::Client::new();
        client
            .delete("http://localhost:8080/api/v1/crates/foo/0.1.0/yank")
            .header("authorization", &session.token)
            .send()
            .await?;
        expected_index_json["yanked"] = serde_json::Value::Bool(true);
        let real_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string("e2e_test_repo/3/f/foo")?)?;
        assert_eq!(real_json, expected_index_json);
    }

    {
        let client = reqwest::Client::new();
        client
            .put("http://localhost:8080/api/v1/crates/foo/0.1.0/unyank")
            .header("authorization", &session.token)
            .send()
            .await?;
        expected_index_json["yanked"] = serde_json::Value::Bool(false);
        let real_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string("e2e_test_repo/3/f/foo")?)?;
        assert_eq!(real_json, expected_index_json);
    }

    Ok(())
}

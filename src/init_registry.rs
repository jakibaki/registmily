use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::path::Path;

use std::process::Command;

use std::env;
use std::fs;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RegistryIndex {
    pub dl: String,
    pub api: String,
}

pub fn setup_registry(git_path: &Path, storage_path: &Path, api_url: &str) {
    let index = RegistryIndex {
        dl: format!("{}/api/v1/dl/{{sha256-checksum}}", api_url),
        api: String::from(api_url),
    };
    fs::create_dir_all(git_path).unwrap();
    let old_dir = env::current_dir().unwrap();
    env::set_current_dir(git_path).unwrap();

    Command::new("git")
        .arg("init")
        .arg("-b")
        .arg("main")
        .arg(".")
        .output()
        .expect("Failed to create repo");

    let json_str = serde_json::to_string(&index).unwrap();

    fs::write("config.json", json_str).unwrap();

    Command::new("git")
        .arg("add")
        .arg("config.json")
        .output()
        .expect("Failed to add config.json");

    Command::new("git")
        .arg("commit")
        .arg("-m")
        .arg("Initialized registry")
        .output()
        .expect("Failed to commit");

    env::set_current_dir(old_dir).unwrap();

    fs::create_dir_all(storage_path).unwrap();
}

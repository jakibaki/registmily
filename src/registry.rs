use axum::extract;
use git2::Repository;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;


use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{thread, time};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::info;

#[derive(Error, Debug)]
pub enum PublishError {
    #[error("checksum invalid")]
    ChecksumInvalid,
    #[error("checksum doesn't match")]
    ChecksumWrong,
    #[error("error happened while writing file: {0}")]
    WriteError(std::io::Error),
    #[error("Invalid json provided")]
    JsonInvalid,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub vers: String,
    pub deps: Vec<PackageDeps>,
    pub cksum: String,
    pub features: PackageFeatures,
    pub yanked: bool,
    pub links: Value,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageDeps {
    pub name: String,
    pub req: String,
    pub features: Vec<String>,
    pub optional: bool,
    pub default_features: bool,
    pub target: Value,
    pub kind: String,
    pub registry: Value,
    pub package: Value,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageFeatures {
    pub extras: Vec<String>,
}

pub type CrateFile = Vec<u8>;

#[derive(Debug, Clone, Copy)]
pub enum Operation {
    Add(i64, i64),
}

pub enum RegistryResponse {
    Add(String),
}

macro_rules! definition {
    ($name:ident, $operation:ident) => {
        pub async fn $name(
            operation: Operation,
            handler: extract::Extension<
                tokio::sync::mpsc::Sender<(
                    Operation,
                    tokio::sync::oneshot::Sender<RegistryResponse>,
                )>,
            >,
        ) -> Result<RegistryResponse, &'static str> {
            let (sender, recv) = tokio::sync::oneshot::channel();
            if handler.send((operation, sender)).await.is_err() {
                return Err("Sender channel died");
            };

            recv.await.map_err(|_| "Oneshot channel died")
        }
    };
}

pub struct Registry {
    repo: Repository,
    repo_path: String,
    storage_location: String,
}

impl Registry {
    pub fn new(git_location: &str, storage_location: &str) -> Self {
        info!("Opening {}", git_location);
        let repo = match Repository::open(git_location) {
            Ok(repo) => repo,
            Err(e) => panic!("failed to open {}", e),
        };

        Self {
            repo,
            repo_path: String::from(git_location),
            storage_location: String::from(storage_location),
        }
    }

    pub fn get_package_git_path(&self, package_name: &str) -> PathBuf {

        // ensure that there can be no path traversal bugs!
        // proper crate name checking needs to be done elsewhere
        // if this ever panics in production it just saved you from a bad vuln :p
        assert!(!package_name.contains("."));

        let package_name = package_name.to_lowercase();
        let mut path = PathBuf::from(&self.repo_path);
        match package_name.len() {
            0 => panic!("invalid crate name passed to get_package_git_path!"),
            1 => path.push("1"),
            2 => path.push("2"),
            3 => {
                path.push("3");
                path.push(&package_name[0..1]);
            }
            _ => {
                path.push(&package_name[0..=1]);
                path.push(&package_name[2..=3]);
            }
        }

        path.push(package_name);

        path
    }

    // TODO: move out of registry struct to apiserver, this does not belong in sync stuff
    fn dl(&self, sha256sum: &str) -> Result<CrateFile, std::io::Error> {
        // todo2: dl endpoint is unauthorized, someone could theoretically find out all mirrored packages by iterating through all sha256 from crates.io,
        // don't really know what to do about that beyond maybe putting some secret string in dl endpoint path

        let mut path = PathBuf::from(&self.storage_location);

        path.push(sha256sum);
        path.set_extension("crate");
        fs::read(path)
    }

    pub fn publish(&self, pkg: &Package, crate_file: &CrateFile) -> Result<(), PublishError> {
        // TODO: validate pkg

        let mut hash = Sha256::new();
        hash.update(crate_file);
        let hash = hash.finalize();
        let hash = hex::encode(hash);
        if hash != str::to_lowercase(&pkg.cksum) {
            return Err(PublishError::ChecksumWrong);
        }

        // TODO: validate owner, need auth token

        // TODO: write and commit package file

        // TODO: validate package name
        let repo_path = self.get_package_git_path(&pkg.name);

        // TODO error handle this lol
        let json_str = serde_json::to_string(pkg).map_err(|_| PublishError::JsonInvalid)?;

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(repo_path)
            .map_err(|err| PublishError::WriteError(err))?;


        let mut cratefile_path = PathBuf::from(&self.storage_location);
        cratefile_path.push(hash);
        cratefile_path.set_extension("crate");

        fs::write(cratefile_path, crate_file).map_err(|err| PublishError::WriteError(err))?;
    

        file.write_fmt(format_args!("{}\n", json_str))
            .map_err(|err| PublishError::WriteError(err))?;

        drop(file);




        Ok(())
    }

    pub fn yank(&self, crate_name: String, version: String, yank_val: bool) {
        let repo_path = self.get_package_git_path(&crate_name);

        let mut set_yanked = false;

        // TODO: fail gracefully if not exists
        let infile = File::open(&repo_path).unwrap();
        let mut outfile = OpenOptions::new().write(true).open(&repo_path).unwrap();
        for line in std::io::BufReader::new(infile).lines() {
            let mut pkg: Package = serde_json::from_str(&line.unwrap()).unwrap();
            if pkg.vers == version {
                pkg.yanked = yank_val;
                set_yanked = true;
            }
            outfile.write_fmt(format_args!("{}\n", serde_json::to_string(&pkg).unwrap())).unwrap();
        }
    }

    // TODO: move out of sync context
    pub fn list_owners(&self, crate_name: String) -> Vec<String> { // todo Result<>
        vec![String::from("emily")]
    }

    pub fn add_owner(&self, owner: String) {
        // how do I store this best? Can this go into the git repo?
        unimplemented!();
    }

    pub fn del_owner(&self, owner: String) {
        unimplemented!();
    }

    // TODO: move this out of sync context
    pub fn find_crates(&self, query: String) {
        unimplemented!()
    }
}

definition!(add, Add);

pub fn handler(
    git_location: &str,
    mut recv: tokio::sync::mpsc::Receiver<(
        Operation,
        tokio::sync::oneshot::Sender<RegistryResponse>,
    )>,
) {
    // The git2-rs library is not thread safe and needs to stay on the same thread at all points in time due to it's use of environment variables

    let registry = Registry::new(git_location, "storage");

    while let Some((op, oneshot_sender)) = recv.blocking_recv() {
        let _ = oneshot_sender.send(match op {
            Operation::Add(a, b) => {
                thread::sleep(time::Duration::from_millis(2000));
                RegistryResponse::Add((a + b).to_string())
            }
        });
    }
}

use axum::extract;
use git2::PackBuilder;
use git2::Repository;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use serde_json::Value;
use sha2::digest::typenum::private::IsLessPrivate;
use sha2::{Digest, Sha256};
use std::env;
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
pub struct PublishPackage {
    pub name: String,
    pub vers: String,
    pub deps: Vec<PublishDep>,
    pub features: Value,
    pub authors: Vec<String>,
    pub description: Value,
    pub documentation: Value,
    pub homepage: Value,
    pub readme: Value,
    pub readme_file: Value,
    pub keywords: Vec<Value>,
    pub categories: Vec<Value>,
    pub license: Value,
    pub license_file: Value,
    pub repository: Value,
    pub badges: Value,
    pub links: Value,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishDep {
    pub name: String,
    pub version_req: String,
    pub features: Vec<String>,
    pub optional: bool,
    pub default_features: bool,
    pub target: Value,
    pub kind: String,
    pub registry: Value,
    pub explicit_name_in_toml: Value,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub vers: String,
    pub deps: Vec<PackageDep>,
    pub cksum: String,
    pub features: Value,
    pub yanked: bool,
    pub links: Value,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageDep {
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

impl From<PublishDep> for PackageDep {
    fn from(pub_dep: PublishDep) -> Self {
        Self {
            name: pub_dep.name,
            req: pub_dep.version_req,
            features: pub_dep.features,
            optional: pub_dep.optional,
            default_features: pub_dep.default_features,
            target: pub_dep.target,
            kind: pub_dep.kind,
            registry: pub_dep.registry,
            package: pub_dep.explicit_name_in_toml,
        }
    }
}

impl Package {
    pub fn from_pub(pub_pkg: PublishPackage, checksum: String) -> Self {
        Self {
            name: pub_pkg.name,
            vers: pub_pkg.vers,
            deps: pub_pkg
                .deps
                .into_iter()
                .map(|x| PackageDep::from(x))
                .collect(),
            cksum: checksum,
            features: pub_pkg.features,
            yanked: false,
            links: pub_pkg.links,
        }
    }
}

pub type CrateFile = Vec<u8>;
pub type SyncSender =
    tokio::sync::mpsc::Sender<(Operation, tokio::sync::oneshot::Sender<RegistryResponse>)>;
pub type SyncReciever =
    tokio::sync::mpsc::Receiver<(Operation, tokio::sync::oneshot::Sender<RegistryResponse>)>;

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

#[derive(Debug, Clone)]
pub enum Operation {
    Publish(Package, CrateFile),
    Yank(String, String, bool),
    AddOwner(String, String),
    DelOwner(String, String),
}

pub enum RegistryResponse {
    Publish(Result<(), PublishError>),
    Yank,
    AddOwner,
    DelOwner,
}

definition!(publish, Publish);
definition!(yank, Yank);
definition!(add_owner, AddOwner);
definition!(del_owner, DelOwner);

pub struct Registry {
    repo: Repository,
    pub repo_path: String,
    storage_location: String,
}

pub fn get_package_git_path(repo_path: &str, package_name: &str) -> PathBuf {
    let mut folder = get_package_git_folder(repo_path, package_name);
    folder.push(package_name);
    folder
}

pub fn get_package_git_folder(repo_path: &str, package_name: &str) -> PathBuf {
    // ensure that there can be no path traversal bugs!
    // proper crate name checking needs to be done elsewhere
    // if this ever panics in production it just saved you from a bad vuln :p
    assert!(!package_name.contains("."));

    let package_name = package_name.to_lowercase();
    let mut path = PathBuf::from(repo_path);
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
    path
}



fn git_credentials_callback(
    _user: &str,
    user_from_url: Option<&str>,
    cred: git2::CredentialType,
) -> Result<git2::Cred, git2::Error> {
    let user = user_from_url.unwrap_or("git");

    if cred.contains(git2::CredentialType::USERNAME) {
        return git2::Cred::username(user);
    }

    let mut ssh_key_path = dirs::home_dir().unwrap();
    ssh_key_path.push(".ssh");
    ssh_key_path.push("id_rsa");

    git2::Cred::ssh_key(user, None, &ssh_key_path, None)
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

    pub fn commit_git_files(&self, paths: Vec<&Path>, message: &str) {
        let mut index = self.repo.index().unwrap();

        for path in paths {
            let path = pathdiff::diff_paths(path, Path::new(&self.repo_path)).unwrap();

            index.add_path(&path.as_path()).unwrap();
        }
        index.write().unwrap();
        let sig = self.repo.signature().unwrap();
        let tree_id = index.write_tree().unwrap();

        let mut parents = Vec::new();
        if let Some(parent) = self.repo.head().ok().map(|h| h.target().unwrap()) {
            parents.push(self.repo.find_commit(parent).unwrap())
        }
        let parents = parents.iter().collect::<Vec<_>>();

        self.repo
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                message,
                &self.repo.find_tree(tree_id).unwrap(),
                &parents,
            )
            .unwrap();

            
        if let Ok(mut remote) = self.repo.find_remote("origin") {
            let mut callbacks = git2::RemoteCallbacks::new();
            callbacks.credentials(git_credentials_callback);

            let mut opts = git2::PushOptions::new();
            opts.remote_callbacks(callbacks);


            remote
                .push(&["refs/heads/main:refs/heads/main"], Some(&mut opts))
                .unwrap();
        } else {
            info!("No remote found");
        }
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

        // TODO: avoid double publish

        // TODO: validate owner, need auth token

        // TODO: validate package name
        let repo_path = get_package_git_path(&self.repo_path, &pkg.name);

        fs::create_dir_all(get_package_git_folder(&self.repo_path, &pkg.name)).unwrap();

        let json_str = serde_json::to_string(pkg).map_err(|_| PublishError::JsonInvalid)?;

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&repo_path)
            .map_err(|err| PublishError::WriteError(err))?;

        let mut cratefile_path = PathBuf::from(&self.storage_location);
        cratefile_path.push(&pkg.cksum);
        cratefile_path.set_extension("crate");

        fs::write(cratefile_path, crate_file).map_err(|err| PublishError::WriteError(err))?;

        file.write_fmt(format_args!("{}\n", json_str))
            .map_err(|err| PublishError::WriteError(err))?;

        drop(file);

        self.commit_git_files(vec![repo_path.as_path()], "added crate");

        Ok(())
    }

    pub fn yank(&self, crate_name: String, version: String, yank_val: bool) {
        let repo_path = get_package_git_path(&self.repo_path, &crate_name);

        let mut set_yanked = false;

        // TODO: fail gracefully if not exists
        let infile = File::open(&repo_path).unwrap();
        let mut outfile = OpenOptions::new().write(true).open(&repo_path).unwrap();
        for line in std::io::BufReader::new(infile).lines() {
            let line = line.unwrap();
            if line == "" {
                continue;
            }

            let mut pkg: Package = serde_json::from_str(&line).unwrap();
            if pkg.vers == version {
                pkg.yanked = yank_val;
                set_yanked = true;
            }
            outfile
                .write_fmt(format_args!("{}\n", serde_json::to_string(&pkg).unwrap()))
                .unwrap();
        }
        drop(outfile);

        let message = if yank_val {"yanked crate"} else {"unyanked crate"};

        self.commit_git_files(vec![repo_path.as_path()], message);
    }

    pub fn add_owner(&self, crate_name: String, owner: &str) {
        let mut path = get_package_git_path(&self.repo_path, &crate_name);
        path.set_extension("owners");
        let cur_owners = match fs::read_to_string(&path) {
            Ok(owners) => owners,
            Err(_) => String::from(""),
        };

        let mut cur_owners: Vec<&str> = cur_owners.split("\n").collect();

        if !cur_owners.contains(&owner) {
            cur_owners.push(owner);
        }

        let cur_owners = cur_owners.join("\n");

        fs::write(&path, cur_owners).unwrap();

        self.commit_git_files(vec![path.as_path()], "added owner to crate");
    }

    pub fn del_owner(&self, crate_name: String, owner: &str) {
        let mut path = get_package_git_path(&self.repo_path, &crate_name);
        path.set_extension("owners");
        let cur_owners = match fs::read_to_string(&path) {
            Ok(owners) => owners,
            Err(_) => String::from(""),
        };

        let mut cur_owners: Vec<&str> = cur_owners.split("\n").collect();

        cur_owners.retain(|x| x != &owner);

        let cur_owners = cur_owners.join("\n");

        fs::write(&path, cur_owners).unwrap();

        self.commit_git_files(vec![path.as_path()], "deleted owner from crate");
    }
}

pub fn handler(git_location: &str, storage_location: &str, mut recv: SyncReciever) {
    // The git2-rs library is not thread safe and needs to stay on the same thread at all points in time due to it's use of environment variables

    let registry = Registry::new(git_location, storage_location);

    while let Some((op, oneshot_sender)) = recv.blocking_recv() {
        let _ = oneshot_sender.send(match op {
            Operation::AddOwner(crate_name, owner) => {
                registry.add_owner(crate_name, &owner);
                RegistryResponse::AddOwner
            }
            Operation::DelOwner(crate_name, owner) => {
                registry.del_owner(crate_name, &owner);
                RegistryResponse::DelOwner
            }
            Operation::Publish(pkg, crate_file) => {
                RegistryResponse::Publish(registry.publish(&pkg, &crate_file))
            }
            Operation::Yank(crate_name, version, yank_val) => {
                registry.yank(crate_name, version, yank_val);
                RegistryResponse::Yank
            }
        });
    }
}

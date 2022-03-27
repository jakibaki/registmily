use git2::Repository;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::info;

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
            deps: pub_pkg.deps.into_iter().map(PackageDep::from).collect(),
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

#[derive(Debug, Clone)]
pub enum Operation {
    Publish(Package, CrateFile),
    Yank(String, String, bool),
}

#[derive(Error, Debug)]
pub enum PublishError {
    // #[error("error happened while writing file: {0}")]
// WriteError(std::io::Error),
// #[error("test")]
// Blubb
}

#[derive(Error, Debug)]
pub enum YankError {
    #[error("Crate not found")]
    CrateNotFound,
}

pub enum RegistryResponse {
    Publish(Result<(), PublishError>),
    Yank(Result<(), YankError>),
}

pub struct Registry {
    repo: Repository,
    pub repo_path: String,
    storage_location: String,
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

pub fn get_package_git_path(repo_path: &str, package_name: &str) -> PathBuf {
    let mut folder = get_package_git_folder(repo_path, package_name);
    folder.push(package_name);
    folder
}

pub fn get_package_git_folder(repo_path: &str, package_name: &str) -> PathBuf {
    // ensure that there can be no path traversal bugs!
    // proper crate name checking needs to be done elsewhere
    // if this ever panics in production it just saved me from a bad vuln :p
    assert!(!package_name.contains('.'));
    assert!(!package_name.contains('/'));

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

            index.add_path(path.as_path()).unwrap();
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

    pub fn publish(&self, pkg: Package, crate_file: &CrateFile) -> Result<(), PublishError> {
        let mut cratefile_path = PathBuf::from(&self.storage_location);
        cratefile_path.push(&pkg.cksum);
        cratefile_path.set_extension("crate");

        fs::write(cratefile_path, crate_file).unwrap();

        let repo_path = get_package_git_path(&self.repo_path, &pkg.name);

        fs::create_dir_all(get_package_git_folder(&self.repo_path, &pkg.name)).unwrap();

        let mut all_published: Vec<Package> = if let Ok(oldfile) = fs::read_to_string(&repo_path) {
            oldfile
                .lines()
                .map(|x| serde_json::from_str(x).unwrap())
                .collect()
        } else {
            vec![]
        };

        all_published.retain(|x| x.vers != pkg.vers);
        all_published.push(pkg);

        let published_strings: Vec<String> = all_published
            .iter()
            .map(|x| serde_json::to_string(x).unwrap())
            .collect();
        fs::write(&repo_path, published_strings.join("\n")).unwrap();

        self.commit_git_files(vec![repo_path.as_path()], "added crate");

        Ok(())
    }

    pub fn yank(
        &self,
        crate_name: String,
        version: String,
        yank_val: bool,
    ) -> Result<(), YankError> {
        let repo_path = get_package_git_path(&self.repo_path, &crate_name);

        let mut found_version = false;
        let mut set_yanked = false;

        // TODO: fail gracefully if not exists
        let mut all_published: Vec<Package> = if let Ok(oldfile) = fs::read_to_string(&repo_path) {
            oldfile
                .lines()
                .map(|x| serde_json::from_str(x).unwrap())
                .collect()
        } else {
            return Err(YankError::CrateNotFound);
        };

        for pkg in all_published.iter_mut() {
            if pkg.vers == version {
                set_yanked = pkg.yanked != yank_val;
                pkg.yanked = yank_val;
                found_version = true;
            }
        }

        let published_strings: Vec<String> = all_published
            .iter()
            .map(|x| serde_json::to_string(x).unwrap())
            .collect();
        fs::write(&repo_path, published_strings.join("\n")).unwrap();

        if found_version {
            if set_yanked {
                let message = if yank_val {
                    "yanked crate"
                } else {
                    "unyanked crate"
                };
                self.commit_git_files(vec![repo_path.as_path()], message);
            }
            Ok(())
        } else {
            Err(YankError::CrateNotFound)
        }
    }
}

pub async fn run_task(
    operation: Operation,
    handler: axum::extract::Extension<SyncSender>,
) -> Result<RegistryResponse, &'static str> {
    let (sender, recv) = tokio::sync::oneshot::channel();
    if handler.send((operation, sender)).await.is_err() {
        return Err("Sender channel died");
    };

    recv.await.map_err(|_| "Oneshot channel died")
}

pub fn handler(git_location: &str, storage_location: &str, mut recv: SyncReciever) {
    // The git2-rs library is not thread safe and needs to stay on the same thread at all points in time due to it's use of environment variables

    let registry = Registry::new(git_location, storage_location);

    while let Some((op, oneshot_sender)) = recv.blocking_recv() {
        let _ = oneshot_sender.send(match op {
            Operation::Publish(pkg, crate_file) => {
                RegistryResponse::Publish(registry.publish(pkg, &crate_file))
            }
            Operation::Yank(crate_name, version, yank_val) => {
                RegistryResponse::Yank(registry.yank(crate_name, version, yank_val))
            }
        });
    }
}

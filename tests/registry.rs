use registmily::registry;
use std::path::Path;

#[test]
pub fn test_registry() {
    let reg = registry::Registry::new("testgit", "storage");

    assert_eq!(
        registry::get_package_git_path(&reg.repo_path, "a").as_path(),
        Path::new("testgit/1/a")
    );
    assert_eq!(
        registry::get_package_git_path(&reg.repo_path, "ab").as_path(),
        Path::new("testgit/2/ab")
    );
    assert_eq!(
        registry::get_package_git_path(&reg.repo_path, "abc").as_path(),
        Path::new("testgit/3/a/abc")
    );
    assert_eq!(
        registry::get_package_git_path(&reg.repo_path, "abcd").as_path(),
        Path::new("testgit/ab/cd/abcd")
    );
    assert_eq!(
        registry::get_package_git_path(&reg.repo_path, "eliseissuperdupercute").as_path(),
        Path::new("testgit/el/is/eliseissuperdupercute")
    );
}

#[tokio::test]
pub async fn async_test_whatever() {}

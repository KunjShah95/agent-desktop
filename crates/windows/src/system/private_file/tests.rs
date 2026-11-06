use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "locality_tests.rs"]
mod locality_tests;
#[path = "owner_tests.rs"]
mod owner_tests;

#[path = "owner_token_tests.rs"]
mod owner_token_tests;
#[path = "path_tests.rs"]
mod path_tests;
#[path = "replace_tests.rs"]
mod replace_tests;

static SCRATCH_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Scratch {
    root: PathBuf,
}
impl Scratch {
    pub(crate) fn new(name: &str) -> Self {
        Self::adopt(std::env::temp_dir().join(format!(
            "agent-desktop-pf-{name}-{}-{:016x}",
            std::process::id(),
            scratch_nonce()
        )))
    }

    pub(super) fn adopt(root: PathBuf) -> Self {
        std::fs::create_dir_all(&root).expect("scratch root must be creatable");
        Self { root }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(super) fn scratch_nonce() -> u64 {
    RandomState::new().hash_one((
        std::process::id(),
        SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now(),
    ))
}

pub(crate) fn create_junction(link: &Path, target: &Path) {
    let status = std::process::Command::new("cmd")
        .arg("/c")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("cmd /c mklink /J must spawn");
    assert!(status.success(), "junction creation must succeed");
    let created = std::fs::symlink_metadata(link).expect("junction metadata must be readable");
    assert!(
        created.file_type().is_symlink(),
        "the planted link must surface as a reparse point"
    );
}

#[test]
fn owns_directory_accepts_a_directory_created_by_this_token() {
    use super::WindowsPrivateFile;
    use agent_desktop_core::PrivateFileOps;

    let root = Scratch::new("owns-directory");

    assert!(
        WindowsPrivateFile::new()
            .owns_directory(root.path())
            .expect("an ordinary private directory is readable"),
        "the state root must accept a directory this token owns"
    );
}

#[test]
fn owns_directory_refuses_a_junction_even_when_its_target_is_owned() {
    use super::WindowsPrivateFile;
    use agent_desktop_core::PrivateFileOps;

    let root = Scratch::new("owns-directory-junction");
    let target = Scratch::new("owns-directory-junction-target");
    let junction = root.path().join("redirect");
    create_junction(&junction, target.path());

    assert!(
        !WindowsPrivateFile::new()
            .owns_directory(&junction)
            .expect("a reparse point is refused, not an I/O failure"),
        "a junction must never satisfy the state-root ownership check"
    );
}

#[test]
fn owns_directory_refuses_a_real_leaf_reached_through_a_junction_ancestor() {
    use super::WindowsPrivateFile;
    use agent_desktop_core::PrivateFileOps;

    let root = Scratch::new("owns-directory-ancestor-junction");
    let target = Scratch::new("owns-directory-ancestor-target");
    let junction = root.path().join("redirect");
    create_junction(&junction, target.path());
    let leaf = junction.join("state");
    std::fs::create_dir(&leaf).expect("a real directory under the junction target");

    assert!(
        !WindowsPrivateFile::new()
            .owns_directory(&leaf)
            .expect("the ancestor chain is readable without following the link"),
        "a state root reached through a junction ancestor must be refused"
    );
}

#[test]
fn owns_directory_accepts_a_missing_state_root_for_lazy_creation() {
    use super::WindowsPrivateFile;
    use agent_desktop_core::PrivateFileOps;

    let root = Scratch::new("owns-directory-missing");
    let missing = root.path().join("not-created-yet");

    assert!(
        WindowsPrivateFile::new()
            .owns_directory(&missing)
            .expect("a missing path is a valid lazy state root"),
        "the ownership check must not reject a state root that will be created later"
    );
}

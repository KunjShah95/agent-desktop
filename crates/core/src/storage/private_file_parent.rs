use std::path::Path;

pub(crate) fn ensure_private(path: &Path) -> std::io::Result<()> {
    crate::storage::private_file_ops::with_active_ops(|ops| ops.ensure_private(path))
}

pub(crate) fn ensure_private_portable(path: &Path) -> std::io::Result<()> {
    ensure_directory_path(path)?;
    let metadata = std::fs::symlink_metadata(path)?;
    if is_link_like(&metadata) || !metadata.is_dir() {
        return Err(super::private_file::permission_denied(
            "private file parent must be a real directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(super::private_file::permission_denied(
                "private file parent is not owned by the effective user",
            ));
        }
        if metadata.mode() & 0o077 != 0 {
            return Err(super::private_file::permission_denied(
                "private file parent is accessible by group or other users",
            ));
        }
    }
    Ok(())
}

pub(crate) fn ensure_user(path: &Path) -> std::io::Result<()> {
    ensure_user_directory_path(path)?;
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_dir() {
        return Err(super::private_file::permission_denied(
            "user output parent must be a directory",
        ));
    }
    Ok(())
}

/// User-named output deliberately keeps the permissive Windows parent rule.
///
/// The destination is a path the caller named, so it may legitimately pass
/// through a reparse point (OneDrive, a mapped folder, a junction the user
/// created). Only private state-root artifacts refuse that traversal; applying
/// the private rule here broke writing a screenshot to a user-named junction.
#[cfg(unix)]
fn ensure_user_directory_path(path: &Path) -> std::io::Result<()> {
    ensure_directory_path(path)
}

#[cfg(not(unix))]
fn ensure_user_directory_path(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

#[cfg(unix)]
fn ensure_directory_path(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};

    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir => {
                return Err(super::private_file::invalid_input(
                    "private file parent must not contain parent traversal",
                ));
            }
            _ => current.push(component.as_os_str()),
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() && metadata.uid() != 0 => {
                return Err(super::private_file::permission_denied(
                    "private file parent path must not contain user-controlled symlinks",
                ));
            }
            Ok(metadata) if !metadata.file_type().is_symlink() && !metadata.is_dir() => {
                return Err(super::private_file::permission_denied(
                    "private file parent path must contain only directories",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match std::fs::DirBuilder::new().mode(0o700).create(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                let metadata = std::fs::symlink_metadata(&current)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(super::private_file::permission_denied(
                        "private file parent creation raced with a non-directory path",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_directory_path(path: &Path) -> std::io::Result<()> {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir => {
                return Err(super::private_file::invalid_input(
                    "private file parent must not contain parent traversal",
                ));
            }
            _ => current.push(component.as_os_str()),
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_like(&metadata) => {
                return Err(super::private_file::permission_denied(
                    "private file parent path must not contain links",
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(super::private_file::permission_denied(
                    "private file parent path must contain only directories",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match std::fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                let metadata = std::fs::symlink_metadata(&current)?;
                if is_link_like(&metadata) || !metadata.is_dir() {
                    return Err(super::private_file::permission_denied(
                        "private file parent creation raced with a non-directory path",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Whether a directory entry redirects name resolution elsewhere. Windows
/// reports junctions as reparse points even when `is_symlink` does not, so the
/// attribute is checked directly on that platform; both are refused because a
/// redirect means the owner of the leaf is not evidence about the named path.
fn is_link_like(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

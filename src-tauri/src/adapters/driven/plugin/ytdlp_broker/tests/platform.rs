#[cfg(unix)]
mod unix {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::super::super::platform::find_approved_binary;

    #[test]
    fn binary_must_resolve_inside_root_with_safe_permissions() {
        let approved = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let valid = executable(approved.path(), "yt-dlp", 0o700);
        let unsafe_binary = executable(outside.path(), "yt-dlp", 0o700);
        let link = approved.path().join("linked-yt-dlp");
        symlink(&unsafe_binary, &link).unwrap();

        let found =
            find_approved_binary(&[link, valid.clone()], &[approved.path().to_path_buf()]).unwrap();

        assert_eq!(found, valid);
    }

    #[test]
    fn group_writable_binary_is_rejected() {
        let approved = tempfile::tempdir().unwrap();
        let binary = executable(approved.path(), "yt-dlp", 0o720);

        assert!(find_approved_binary(&[binary], &[approved.path().to_path_buf()]).is_err());
    }

    fn executable(directory: &std::path::Path, name: &str, mode: u32) -> std::path::PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }
}

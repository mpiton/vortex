#[cfg(unix)]
mod unix {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::super::super::platform::{controlled_path_with_roots, find_approved_binary};

    #[test]
    fn binary_must_resolve_inside_root_with_safe_permissions() {
        let approved = tempfile::tempdir().unwrap();
        std::fs::set_permissions(approved.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let valid = executable(approved.path(), "yt-dlp", 0o700);
        let unsafe_binary = executable(outside.path(), "yt-dlp", 0o700);
        let link_dir = approved.path().join("linked");
        std::fs::create_dir(&link_dir).unwrap();
        std::fs::set_permissions(&link_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let link = link_dir.join("yt-dlp");
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

    #[test]
    fn missing_binary_error_keeps_install_remediation() {
        let error = find_approved_binary(&[], &[]).expect_err("missing yt-dlp");

        assert!(error.to_string().contains("~/.local/bin/yt-dlp"));
    }

    #[test]
    fn binary_below_group_writable_parent_is_rejected() {
        let approved = tempfile::tempdir().unwrap();
        let writable_parent = approved.path().join("bin");
        std::fs::create_dir(&writable_parent).unwrap();
        std::fs::set_permissions(&writable_parent, std::fs::Permissions::from_mode(0o770)).unwrap();
        let binary = executable(&writable_parent, "yt-dlp", 0o700);

        assert!(find_approved_binary(&[binary], &[approved.path().to_path_buf()]).is_err());
    }

    #[test]
    fn controlled_path_does_not_add_unselected_user_candidate_directories() {
        let approved = tempfile::tempdir().unwrap();
        std::fs::set_permissions(approved.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let binary = executable(approved.path(), "yt-dlp", 0o700);
        let path = controlled_path_with_roots(&binary, &[approved.path().to_path_buf()]).unwrap();
        let entries = std::env::split_paths(&path).collect::<Vec<_>>();

        assert!(entries.contains(&approved.path().to_path_buf()));
        if let Some(home) = dirs::home_dir() {
            assert!(!entries.contains(&home.join(".local/bin")));
            assert!(!entries.contains(&home.join(".nix-profile/bin")));
        }
    }

    fn executable(directory: &std::path::Path, name: &str, mode: u32) -> std::path::PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }
}

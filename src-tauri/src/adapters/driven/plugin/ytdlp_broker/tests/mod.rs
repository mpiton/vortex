mod audio;
mod legacy;
mod platform;
mod process;
mod request;

fn private_root(temp: &std::path::Path) -> std::path::PathBuf {
    let managed = temp.join("vortex-downloads");
    let request = managed.join("request-test");
    std::fs::create_dir_all(&request).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&managed, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&request, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    request
}

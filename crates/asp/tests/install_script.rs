//! Installer smoke tests. These fake network/platform inputs and never call
//! GitHub.

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    fn install_script() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../install.sh")
            .canonicalize()
            .unwrap()
    }

    fn write_executable(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    fn run_installer(home: &Path, curl: &Path, os: &str, arch: &str) -> Output {
        Command::new("sh")
            .arg(install_script())
            .env("HOME", home)
            .env("ASP_INSTALL_DIR", home.join("bin"))
            .env("ASP_CURL", curl)
            .env("ASP_INSTALL_OS", os)
            .env("ASP_INSTALL_ARCH", arch)
            .output()
            .expect("installer runs")
    }

    #[test]
    fn installer_reports_unsupported_platform_with_hint() {
        let tmp = tempfile::tempdir().unwrap();
        let curl = tmp.path().join("curl");
        write_executable(&curl, "#!/bin/sh\nexit 0\n");

        let out = run_installer(tmp.path(), &curl, "Plan9", "x86_64");

        assert!(!out.status.success());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("unsupported OS: Plan9"), "{stderr}");
        assert!(stderr.contains("hint:"), "{stderr}");
        assert!(stderr.contains("cargo install"), "{stderr}");
    }

    #[test]
    fn installer_reports_offline_release_lookup_with_hint() {
        let tmp = tempfile::tempdir().unwrap();
        let curl = tmp.path().join("curl");
        write_executable(&curl, "#!/bin/sh\nexit 7\n");

        let out = run_installer(tmp.path(), &curl, "Linux", "x86_64");

        assert!(!out.status.success());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("could not reach GitHub releases API"),
            "{stderr}"
        );
        assert!(stderr.contains("ASP_INSTALL_VERSION"), "{stderr}");
    }

    #[test]
    fn installer_reports_checksum_mismatch_with_hint() {
        let tmp = tempfile::tempdir().unwrap();
        let curl = tmp.path().join("curl");
        write_executable(
            &curl,
            r#"#!/bin/sh
out=
last=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) shift; out="$1" ;;
    *) last="$1" ;;
  esac
  shift
done
case "$last" in
  *releases/latest) printf '{"tag_name":"v9.9.9"}'; exit 0 ;;
  *.sha256) printf '0000000000000000000000000000000000000000000000000000000000000000  archive\n' > "$out"; exit 0 ;;
  *.tar.gz) printf 'not the expected archive' > "$out"; exit 0 ;;
esac
exit 1
"#,
        );

        let out = run_installer(tmp.path(), &curl, "Linux", "x86_64");

        assert!(!out.status.success());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("checksum mismatch"), "{stderr}");
        assert!(stderr.contains("do not run this archive"), "{stderr}");
        assert!(!tmp.path().join("bin/asp").exists());
    }
}

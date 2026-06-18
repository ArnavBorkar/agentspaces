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
        installer_command(home, curl, os, arch)
            .output()
            .expect("installer runs")
    }

    fn installer_command(home: &Path, curl: &Path, os: &str, arch: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.arg(install_script())
            .env("HOME", home)
            .env("ASP_INSTALL_DIR", home.join("bin"))
            .env("ASP_CURL", curl)
            .env("ASP_INSTALL_OS", os)
            .env("ASP_INSTALL_ARCH", arch);
        cmd
    }

    fn create_release_archive(root: &Path) -> PathBuf {
        let payload_dir = root.join("payload");
        fs::create_dir_all(&payload_dir).unwrap();
        write_executable(
            &payload_dir.join("asp"),
            "#!/bin/sh\nprintf 'fake asp\\n'\n",
        );

        let archive = root.join("asp.tar.gz");
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&payload_dir)
            .arg("asp")
            .status()
            .expect("tar runs");
        assert!(status.success(), "tar should create installer fixture");
        archive
    }

    fn write_successful_curl(path: &Path) {
        write_executable(
            path,
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
asset="asp-v9.9.9-${ASP_EXPECTED_TARGET}.tar.gz"
case "$last" in
  *releases/latest) printf '{"tag_name":"v9.9.9"}'; exit 0 ;;
  *"/$asset") cp "$ASP_FAKE_ARCHIVE" "$out"; exit 0 ;;
  *"/$asset.sha256")
    if command -v sha256sum >/dev/null 2>&1; then
      sum="$(sha256sum "$ASP_FAKE_ARCHIVE" | cut -d ' ' -f1)"
    else
      sum="$(shasum -a 256 "$ASP_FAKE_ARCHIVE" | cut -d ' ' -f1)"
    fi
    printf '%s  %s\n' "$sum" "$asset" > "$out"
    exit 0
    ;;
esac
printf 'unexpected URL: %s\n' "$last" >&2
exit 42
"#,
        );
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

    #[test]
    fn installer_selects_supported_release_assets() {
        let cases = [
            ("Darwin", "arm64", "aarch64-apple-darwin"),
            ("Darwin", "x86_64", "x86_64-apple-darwin"),
            ("Linux", "x86_64", "x86_64-unknown-linux-musl"),
            ("Linux", "aarch64", "aarch64-unknown-linux-musl"),
        ];

        for (os, arch, target) in cases {
            let tmp = tempfile::tempdir().unwrap();
            let archive = create_release_archive(tmp.path());
            let curl = tmp.path().join("curl");
            write_successful_curl(&curl);

            let out = installer_command(tmp.path(), &curl, os, arch)
                .env("ASP_FAKE_ARCHIVE", &archive)
                .env("ASP_EXPECTED_TARGET", target)
                .output()
                .expect("installer runs");

            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success(),
                "install failed for {os}/{arch} => {target}\nstdout:\n{stdout}\nstderr:\n{stderr}"
            );
            assert!(
                stderr.contains(&format!("downloading asp-v9.9.9-{target}.tar.gz...")),
                "{stderr}"
            );
            assert!(tmp.path().join("bin/asp").exists(), "{stderr}");
        }
    }
}

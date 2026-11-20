use bamabi_host::SignatureVerificationError;
use bamabi_host::prelude::*;
use bevy_app::App;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

struct TempWorkDir {
    path: PathBuf,
}

impl TempWorkDir {
    fn new(prefix: &str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "bamabi_xtask_sign_test_{prefix}_{}_{}",
            std::process::id(),
            id
        ));
        std::fs::create_dir_all(&path).expect("create temp work dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempWorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct TempDylibCopy {
    path: PathBuf,
    _dir: TempWorkDir,
}

impl TempDylibCopy {
    fn new(src: &Path, stem: &str) -> Self {
        let dir = TempWorkDir::new(stem);
        let prefix = if cfg!(windows) { "" } else { "lib" };
        let ext = std::env::consts::DLL_EXTENSION;
        let path = dir.path().join(format!("{prefix}{stem}.{ext}"));
        std::fs::copy(src, &path).expect("copy dylib to temp path");
        Self { path, _dir: dir }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

fn dylib_filename(stem: &str) -> String {
    let prefix = if cfg!(windows) { "" } else { "lib" };
    let ext = std::env::consts::DLL_EXTENSION;
    format!("{prefix}{stem}.{ext}")
}

fn resolve_or_stage_include_dir(showcase_root: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os("BAM_INCLUDE_DIR").filter(|v| !v.is_empty()) {
        let inc = PathBuf::from(dir);
        assert!(
            inc.join("bamabi.h").is_file(),
            "BAM_INCLUDE_DIR {} lacks bamabi.h",
            inc.display()
        );
        return inc;
    }
    if let Some(wire) = std::env::var_os("BAM_WIRE_DIR").filter(|v| !v.is_empty()) {
        let inc = PathBuf::from(wire).join("include");
        if inc.join("bamabi.h").is_file() {
            return inc;
        }
    }
    if let Some(parent) = showcase_root.parent() {
        let sibling_inc = parent.join("bamabi-wire/include");
        if sibling_inc.join("bamabi.h").is_file() {
            return sibling_inc;
        }
    }

    let target_inc = showcase_root.join("target/include");
    let header_path = target_inc.join("bamabi.h");
    if !header_path.is_file() {
        std::fs::create_dir_all(&target_inc).expect("create target/include");
        let rendered = bamabi_sdk_gen::render_c99_header().expect("render bamabi.h");
        let tmp = target_inc.join(format!(".bamabi.h.{}.tmp", std::process::id()));
        std::fs::write(&tmp, rendered.as_bytes()).expect("write temp bamabi.h");
        if std::fs::rename(&tmp, &header_path).is_err() && !header_path.is_file() {
            let _ = std::fs::remove_file(&tmp);
            panic!("failed to install {}", header_path.display());
        }
        let _ = std::fs::remove_file(&tmp);
    }
    target_inc
}

fn ensure_asymmetric_c_mod_dylib() -> (PathBuf, &'static str) {
    let showcase_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bamabi-showcase root");
    let asym_fname = dylib_filename("asymmetric_c_mod");
    let ex3_fname = dylib_filename("example_03_c_mod");

    if let Some(dir) = std::env::var_os("BAM_FIXTURE_DIR").filter(|v| !v.is_empty()) {
        let dir_path = PathBuf::from(dir);
        let asym_cand = dir_path.join(&asym_fname);
        if asym_cand.is_file() {
            return (asym_cand, "asymmetric_c_mod");
        }
        let ex3_cand = dir_path.join(&ex3_fname);
        if ex3_cand.is_file() {
            return (ex3_cand, "example_03_c_mod");
        }
    }
    let showcase_asym = showcase_root.join("target/fixtures").join(&asym_fname);
    if showcase_asym.is_file() {
        return (showcase_asym, "asymmetric_c_mod");
    }
    let showcase_ex3 = showcase_root.join("target/fixtures").join(&ex3_fname);
    if showcase_ex3.is_file() {
        return (showcase_ex3, "example_03_c_mod");
    }

    let core_root = std::env::var_os("BAM_CORE_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| showcase_root.parent().map(|p| p.join("bamabi-core")));
    if let Some(ref core) = core_root {
        let core_fixture = core.join("target/fixtures").join(&asym_fname);
        if core_fixture.is_file() {
            return (core_fixture, "asymmetric_c_mod");
        }
    }

    let (c_src, extra_inc, out_fname, staged_dest, principal) = if let Some(ref core) = core_root
        && core.join("tests/asymmetric_c_mod/mod.c").is_file()
    {
        (
            core.join("tests/asymmetric_c_mod/mod.c"),
            Some(core.join("tests/asymmetric_c_mod/include")),
            asym_fname,
            showcase_asym,
            "asymmetric_c_mod",
        )
    } else {
        (
            showcase_root.join("examples/03_cross_language_interop/c_mod/mod.c"),
            None,
            ex3_fname,
            showcase_ex3,
            "example_03_c_mod",
        )
    };
    assert!(
        c_src.is_file(),
        "missing C mod source at {}",
        c_src.display()
    );

    let wire_inc = resolve_or_stage_include_dir(showcase_root);
    let out_dir = showcase_root.join("target/fixtures");
    std::fs::create_dir_all(&out_dir).expect("create target/fixtures");
    let stage = TempWorkDir::new("build_sign_test_c_mod");
    let staged_out = stage.path().join(&out_fname);
    let candidates: Vec<std::ffi::OsString> = if let Some(cc) = std::env::var_os("CC") {
        vec![cc]
    } else if cfg!(windows) {
        ["clang", "clang-cl", "cl.exe", "cl", "gcc", "cc"]
            .into_iter()
            .map(Into::into)
            .collect()
    } else {
        ["cc", "clang", "gcc"].into_iter().map(Into::into).collect()
    };

    for cc in &candidates {
        let stem = Path::new(cc)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let is_msvc = stem.eq_ignore_ascii_case("cl") || stem.eq_ignore_ascii_case("clang-cl");
        let mut cmd = Command::new(cc);
        if is_msvc {
            cmd.args(["/nologo", "/LD", "/TC", "/std:c11", "/O2"])
                .arg(format!("/I{}", wire_inc.display()));
            if let Some(ref inc) = extra_inc {
                cmd.arg(format!("/I{}", inc.display()));
            }
            cmd.arg(format!("/Fe:{}", staged_out.display()))
                .arg(format!("/Fo:{}/", stage.path().display()))
                .arg(&c_src);
        } else {
            cmd.args(["-std=c99", "-O2", "-shared"]);
            if !cfg!(windows) {
                cmd.arg("-fPIC");
            }
            cmd.arg("-I").arg(&wire_inc);
            if let Some(ref inc) = extra_inc {
                cmd.arg("-I").arg(inc);
            }
            cmd.arg("-o").arg(&staged_out).arg(&c_src);
        }
        if let Ok(out) = cmd.output() {
            assert!(
                out.status.success(),
                "C compiler {:?} failed: stderr={}",
                cc,
                String::from_utf8_lossy(&out.stderr)
            );
            let tmp = out_dir.join(format!(".{out_fname}.{}.tmp", std::process::id()));
            std::fs::copy(&staged_out, &tmp).expect("stage fixture copy");
            if std::fs::rename(&tmp, &staged_dest).is_err() && !staged_dest.is_file() {
                let _ = std::fs::remove_file(&tmp);
                panic!("failed to install {}", staged_dest.display());
            }
            let _ = std::fs::remove_file(&tmp);
            return (staged_dest, principal);
        }
    }
    panic!("no working C compiler found to build {principal}");
}

fn run_xtask_sign(args: &[&str]) -> std::process::Output {
    let showcase_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bamabi-showcase root");
    let xtask_bin = env!("CARGO_BIN_EXE_xtask");
    Command::new(xtask_bin)
        .current_dir(showcase_root)
        .arg("sign")
        .args(args)
        .output()
        .expect("execute xtask sign")
}

fn setup_host_app(policy: ModTrustPolicy) -> App {
    let mut app = App::new();
    app.add_plugins(AbiModPlugin::with_trust_policy(policy));
    app
}

#[test]
fn xtask_sign_cli_end_to_end_with_krl_workflow() {
    let (c_dylib, principal_name) = ensure_asymmetric_c_mod_dylib();
    let mod_copy = TempDylibCopy::new(&c_dylib, "xtask_cli_mod");
    let work = TempWorkDir::new("xtask_sign");

    let ca_prefix = work.path().join("ca");
    let signer_prefix = work.path().join("signer");
    let ca_pub = work.path().join("ca.pub");
    let signer_pub = work.path().join("signer.pub");
    let cert_path = work.path().join("signer-cert.pub");
    let krl_path = work.path().join("revocations.krl");

    let assert_ok = |label: &str, out: std::process::Output| {
        assert!(
            out.status.success(),
            "{label} failed: stdout={}, stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    };

    assert_ok(
        "keygen ca",
        run_xtask_sign(&[
            "keygen",
            "--out",
            ca_prefix.to_str().unwrap(),
            "--comment",
            "ci-ca",
        ]),
    );
    assert_ok(
        "keygen signer",
        run_xtask_sign(&["keygen", "--out", signer_prefix.to_str().unwrap()]),
    );
    assert_ok(
        "cert",
        run_xtask_sign(&[
            "cert",
            "--ca",
            ca_prefix.to_str().unwrap(),
            "--pubkey",
            signer_pub.to_str().unwrap(),
            "--id",
            "xtask-ci-signer",
            "--principal",
            principal_name,
            "--serial",
            "77",
            "--out",
            cert_path.to_str().unwrap(),
        ]),
    );
    assert_ok(
        "file",
        run_xtask_sign(&[
            "file",
            "--key",
            signer_prefix.to_str().unwrap(),
            "--cert",
            cert_path.to_str().unwrap(),
            mod_copy.path().to_str().unwrap(),
        ]),
    );
    assert_ok(
        "verify ok",
        run_xtask_sign(&[
            "verify",
            "--ca",
            ca_pub.to_str().unwrap(),
            "--principal",
            principal_name,
            mod_copy.path().to_str().unwrap(),
        ]),
    );
    assert_ok(
        "krl",
        run_xtask_sign(&[
            "krl",
            "--out",
            krl_path.to_str().unwrap(),
            "--version",
            "3",
            "--ca",
            ca_pub.to_str().unwrap(),
            "--revoke-serial",
            "77",
            "--sign-key",
            ca_prefix.to_str().unwrap(),
        ]),
    );

    let revoked_verify = run_xtask_sign(&[
        "verify",
        "--ca",
        ca_pub.to_str().unwrap(),
        "--krl",
        krl_path.to_str().unwrap(),
        mod_copy.path().to_str().unwrap(),
    ]);
    assert!(
        !revoked_verify.status.success(),
        "xtask verify --krl must fail on revoked serial 77"
    );

    let host_policy = ModTrustPolicy::disabled()
        .add_ca_file(&ca_pub)
        .unwrap()
        .with_manifest_name_principal_binding(true)
        .add_signed_krl_file(&krl_path, None)
        .expect("load signed KRL produced by xtask sign krl");
    assert_eq!(host_policy.applied_krl_version(), Some(3));
    let mut app = setup_host_app(host_policy);
    assert!(matches!(
        app.load_mod(mod_copy.path()),
        Err(ModLoadError::SignatureVerification {
            source: SignatureVerificationError::CertificateSerialRevoked { serial: 77 },
            ..
        })
    ));
}

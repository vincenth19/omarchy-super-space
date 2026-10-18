//! pdftotext runs at the lowest CPU and I/O priority. A test binary of its own: it puts a fake
//! pdftotext first on PATH, which must not reach the real PDF test.
use std::{env, fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn pdftotext_runs_idle() {
    if Command::new("ionice").output().is_err() {
        eprintln!("skipped: no ionice");
        return;
    }
    let h = env::temp_dir().join(format!("ss-files-idle-{}", std::process::id()));
    let _ = fs::remove_dir_all(&h);
    for d in ["bin", "Documents"] {
        fs::create_dir_all(h.join(d)).unwrap();
    }
    fs::write(h.join("bin/pdftotext"), "#!/bin/sh\necho \"nice $(nice) io $(ionice)\"\n").unwrap();
    fs::set_permissions(h.join("bin/pdftotext"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(h.join("Documents/a.pdf"), "%PDF").unwrap();
    // SAFETY: the only test in this binary, so no other thread reads the environment.
    unsafe { env::set_var("PATH", format!("{}:{}", h.join("bin").display(), env::var("PATH").unwrap())) };
    let d = ss_files::doc(&h.join("Documents/a.pdf"), &h).unwrap();
    let texts: Vec<_> = d.parts.iter().map(|p| &p.text).collect();
    assert_eq!(texts, ["nice 19 io idle"]);
}

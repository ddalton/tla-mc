//! `P(Succ) == INSTANCE M`: a definition with parameters whose body is
//! INSTANCE is a parameterized instance (MCReachabilityTest's graphs). The
//! parser rejected it with "unexpected keyword"; now it names what is
//! unsupported, so the refusal says what tla-mc cannot do, not that the
//! spec is miswritten.

use std::path::Path;
use std::process::Command;

const SPEC: &str = r#"---- MODULE ParamToy ----
CONSTANT Succ
P(Succ) == INSTANCE Naturals
====
"#;

#[test]
fn a_parameterized_instance_is_refused_by_name() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("param-toy");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ParamToy.tla"), SPEC).unwrap();
    std::fs::write(dir.join("ParamToy.cfg"), "INIT Init\nNEXT Next\n").unwrap();

    let o = Command::new(env!("CARGO_BIN_EXE_tla-mc"))
        .current_dir(&dir)
        .args(["-config", "ParamToy.cfg", "ParamToy.tla"])
        .output()
        .expect("spawn");
    let out = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    assert!(out.contains("parameterized instances"), "{out}");
    assert!(!out.contains("unexpected keyword"), "{out}");
    assert_eq!(o.status.code(), Some(2), "an error exits 2: {out}");
}

//! A cfg naming `POSTCONDITION` (a property of the finished trace) is
//! refused: nothing here models it, and checking without it would hold
//! tla-mc to less than the cfg asks. Before this, it was dropped and the
//! model was checked silently without it. `ALIAS` only renames the operators
//! a trace is written with, so it changes nothing that is checked: the model
//! is checked, with a note that traces are printed without it.

use std::path::Path;
use std::process::Command;

const SPEC: &str = r#"---- MODULE AliasToy ----
VARIABLE x
Init == x = 1
Next == x' = x
Alias == [x |-> x]
TraceAccepted == TRUE
====
"#;

fn run(dir: &Path, cfg: &str) -> (Option<i32>, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_tla-mc"))
        .current_dir(dir)
        .args(["-config", cfg, "AliasToy.tla"])
        .output()
        .expect("spawn");
    (o.status.code(), String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr))
}

#[test]
fn a_postcondition_is_refused_and_an_alias_is_noted() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("alias-toy");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("AliasToy.tla"), SPEC).unwrap();

    // ALIAS, with the name on the line after the keyword, as real cfgs write
    // it: the model is checked, and the note names the alias
    std::fs::write(dir.join("a.cfg"), "INIT Init\nNEXT Next\nALIAS\nAlias\n").unwrap();
    let (code, out) = run(&dir, "a.cfg");
    assert!(out.contains("ALIAS Alias ignored"), "{out}");
    assert!(out.contains("No error has been found"), "the model is checked: {out}");
    assert_eq!(code, Some(0), "{out}");

    // POSTCONDITION, with the name on the same line: refused, exit 2
    std::fs::write(dir.join("b.cfg"), "INIT Init\nNEXT Next\nPOSTCONDITION TraceAccepted\n").unwrap();
    let (code, out) = run(&dir, "b.cfg");
    assert!(out.contains("POSTCONDITION is not supported"), "{out}");
    assert!(!out.contains("No error has been found"), "the model must not be checked: {out}");
    assert_eq!(code, Some(2), "an error exits 2: {out}");
}

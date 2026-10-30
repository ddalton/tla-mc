//! An invariant that is ENABLED of an action and names no variable itself
//! (`NoDeadlock == ENABLED Next`, as tlaplus/Examples' DiningPhilosophers
//! writes it) is a state predicate, checked in every state. Before this, the
//! constant folding at load took it for a constant, because the walk over an
//! expression's parts does not enter an action, and ran `Next` against no
//! state: tla-mc panicked (index out of bounds) before checking anything.

use std::path::Path;
use std::process::Command;

const SPEC: &str = r#"---- MODULE EnabledInv ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x < 3 /\ x' = x + 1
Spin == x' = (x + 1) % 3
NoDeadlock == ENABLED Next
CanSpin == ENABLED Spin
====
"#;

fn run(dir: &Path, cfg: &str, engine: &str) -> (Option<i32>, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_tla-mc"))
        .current_dir(dir)
        .args(["-engine", engine, "-workers", "2", "-config", cfg, "EnabledInv.tla"])
        .output()
        .expect("spawn");
    (o.status.code(), String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr))
}

#[test]
fn an_enabled_invariant_is_checked_in_every_state() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("enabled-inv");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("EnabledInv.tla"), SPEC).unwrap();
    // Spin can always step: the invariant holds in all three states
    std::fs::write(dir.join("holds.cfg"), "INIT Init\nNEXT Spin\nINVARIANT CanSpin\n").unwrap();
    // Next stops at x = 3: the invariant fails there, not at load
    std::fs::write(dir.join("fails.cfg"), "INIT Init\nNEXT Next\nCHECK_DEADLOCK FALSE\nINVARIANT NoDeadlock\n").unwrap();

    for engine in ["interp", "closure"] {
        let (code, out) = run(&dir, "holds.cfg", engine);
        assert!(out.contains("3 distinct states found"), "{engine}: {out}");
        assert_eq!(code, Some(0), "{engine}: {out}");

        let (code, out) = run(&dir, "fails.cfg", engine);
        assert!(out.contains("Invariant NoDeadlock is violated"), "{engine}: {out}");
        assert_eq!(code, Some(12), "{engine}: a violation exits 12: {out}");
    }
}

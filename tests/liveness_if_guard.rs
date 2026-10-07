//! `IF c THEN A ELSE B` in a property, c a state predicate and A, B
//! temporal (tlaplus/Examples CoffeeCan's TerminationHypothesis): c is read
//! in a behavior's first state, so A must hold of the behaviors from initial
//! states where c holds and B of the others. The toy starts at 0 or 1 and
//! climbs by 2 to 4 or 5, so the parity of the start decides where it ends.
//! The right property holds; each wrong branch, and the two swapped, is
//! caught — a guard that sent every behavior to one side, or ignored the
//! branch, would let one of them through.

use std::path::Path;
use std::process::Command;

const SPEC: &str = r#"---- MODULE IfToy ----
EXTENDS Naturals
VARIABLE x
Init == x \in {0, 1}
Next == x < 4 /\ x' = x + 2
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Right == IF x % 2 = 0 THEN <>(x = 4) ELSE <>(x = 5)
Swapped == IF x % 2 = 0 THEN <>(x = 5) ELSE <>(x = 4)
ThenWrong == IF x % 2 = 0 THEN <>(x = 5) ELSE <>(x = 5)
ElseWrong == IF x % 2 = 0 THEN <>(x = 4) ELSE <>(x = 4)
====
"#;

fn check(dir: &Path, prop: &str) -> String {
    std::fs::write(dir.join(format!("{prop}.cfg")), format!("SPECIFICATION Spec\nCHECK_DEADLOCK FALSE\nPROPERTY {prop}\n")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_tla-mc"))
        .current_dir(dir)
        .args(["-workers", "2", "-metadir", dir.join(format!("md-{prop}")).to_str().unwrap(), "-config", &format!("{prop}.cfg"), "IfToy.tla"])
        .output()
        .expect("spawn");
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

#[test]
fn an_if_in_a_property_checks_each_branch_on_the_behaviors_it_governs() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("if-toy");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("IfToy.tla"), SPEC).unwrap();

    let out = check(&dir, "Right");
    assert!(out.contains("No error has been found"), "Right must hold: {out}");
    assert!(out.contains("6 distinct states found"), "0,2,4 and 1,3,5: {out}");
    for wrong in ["Swapped", "ThenWrong", "ElseWrong"] {
        let out = check(&dir, wrong);
        assert!(out.contains(&format!("Temporal property {wrong} is violated")), "{wrong} must be caught: {out}");
    }
}

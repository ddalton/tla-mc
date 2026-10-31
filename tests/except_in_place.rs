//! EXCEPT changes an entry in place when nothing else holds the function
//! (`Value::update`). Every value something else still reads must keep its
//! old entries: the state being stepped from, an EXCEPT's own base in its
//! later clauses, a LET-bound value used again. Each engine checks a spec
//! whose invariant gives every variable in closed form, so an entry changed
//! where it was shared fails it.
//!
//! Builds a generated crate (debug), so it takes a minute the first time.

use std::path::{Path, PathBuf};
use std::process::Command;

const SPEC: &str = r#"---- MODULE ExceptToy ----
EXTENDS Naturals
VARIABLES n, q, f, g, s, r, p
Init == /\ n = 0
        /\ q = [i \in 1..3 |-> 0]
        /\ f = [i \in 1..3 |-> 0]
        /\ g = [i \in 1..2 |-> [j \in 1..2 |-> 0]]
        /\ s = <<0, 0>>
        /\ r = [a |-> 0, b |-> 0]
        /\ p = <<[i \in 1..2 |-> 0], [i \in 1..2 |-> 0]>>
Next == /\ n < 4
        /\ n' = n + 1
        \* built from f before f' is: f must keep its entries
        /\ q' = [f EXCEPT ![1] = 1000]
        \* @ is the old entry; a later clause reads f, not the f being built
        /\ f' = [f EXCEPT ![1] = @ + 1, ![2] = f[1] + 10, ![3] = @ + f[1]]
        \* nested, the same path twice, then g[1] as it was
        /\ g' = [g EXCEPT ![1][2] = @ + 1, ![1][2] = @ * 2, ![2] = [g[1] EXCEPT ![1] = 7]]
        \* 3 is outside the domain: no change
        /\ s' = [s EXCEPT ![1] = @ + 1, ![3] = 99]
        /\ r' = [r EXCEPT !.a = @ + 1, !.b = r.a]
        \* h is read again after the EXCEPT on it
        /\ p' = LET h == [i \in 1..2 |-> n] IN <<[h EXCEPT ![1] = 100], h>>
Two == <<1, 2, 4, 8, 16, 32>>
Prev == IF n = 0 THEN 0 ELSE n - 1
Inv == /\ q[1] = (IF n = 0 THEN 0 ELSE 1000)
       /\ f[1] = n /\ f[2] = (IF n = 0 THEN 0 ELSE n + 9) /\ f[3] = (n * Prev) \div 2
       /\ g[1][1] = 0 /\ g[1][2] = Two[n + 2] - 2
       /\ g[2][1] = (IF n = 0 THEN 0 ELSE 7) /\ g[2][2] = (IF n = 0 THEN 0 ELSE Two[n + 1] - 2)
       /\ s[1] = n /\ s[2] = 0 /\ DOMAIN s = 1..2
       /\ r.a = n /\ r.b = Prev
       /\ p[1][1] = (IF n = 0 THEN 0 ELSE 100) /\ p[1][2] = Prev /\ p[2][1] = Prev /\ p[2][2] = Prev
====
"#;

const CFG: &str = "INIT Init\nNEXT Next\nCHECK_DEADLOCK FALSE\nINVARIANT Inv\n";

fn run(cmd: &mut Command) -> String {
    let out = cmd.output().expect("spawn");
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

fn generated(dir: &Path) -> PathBuf {
    let crate_dir = dir.join("gen");
    let out = run(Command::new(env!("CARGO_BIN_EXE_tla-mc"))
        .current_dir(dir)
        .args(["-codegen", crate_dir.to_str().unwrap(), "-config", "ExceptToy.cfg", "ExceptToy.tla"]));
    let main = std::fs::read_to_string(crate_dir.join("src/main.rs")).unwrap_or_else(|_| panic!("codegen failed: {out}"));
    assert!(main.contains("upd("), "the EXCEPTs must be compiled, not left to the interpreter");
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("codegen-except");
    let out = run(Command::new(env!("CARGO"))
        .current_dir(&crate_dir)
        .env("CARGO_TARGET_DIR", &target)
        .args(["build", "--offline", "--quiet"]));
    let bin = target.join("debug/tlcgen-excepttoy");
    assert!(bin.exists(), "the generated crate must build: {out}");
    bin
}

#[test]
fn except_leaves_every_value_still_read_unchanged() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("except-toy");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ExceptToy.tla"), SPEC).unwrap();
    std::fs::write(dir.join("ExceptToy.cfg"), CFG).unwrap();
    let tla_mc = PathBuf::from(env!("CARGO_BIN_EXE_tla-mc"));
    let bin = generated(&dir);
    for (who, exe, engine) in [("interp", &tla_mc, Some("interp")), ("closure", &tla_mc, Some("closure")), ("generated", &bin, None)] {
        let mut cmd = Command::new(exe);
        cmd.current_dir(&dir);
        if let Some(e) = engine {
            cmd.args(["-engine", e]);
        }
        let out = run(cmd.args(["-workers", "2", "-metadir", dir.join(format!("md-{who}")).to_str().unwrap(), "-config", "ExceptToy.cfg", "ExceptToy.tla"]));
        assert!(out.contains("No error has been found"), "{who}: {out}");
        assert!(out.contains(" 5 distinct states found"), "{who}: {out}");
    }
}

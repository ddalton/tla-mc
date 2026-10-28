# Recursion in `Init` and in actions — design

Status: proposed, not implemented (2026-10-28).

## What works today, and what is refused

Recursion works on all three engines (`interp`, `closure`, `-compile`) for
top-level `RECURSIVE` operators (mutual recursion included), `LET RECURSIVE`,
and recursive function definitions (`f[n \in S] == ... f[n-1] ...`), wherever
they are used as values: invariants, guards, `x' = Op(...)` in `Next`.

Two positions are refused when the spec loads, with the misleading message
"Op recurses; only top-level RECURSIVE operators are supported":

1. **A recursive operator that reads a state variable, called from `Init`**
   (`Init == x = 0 /\ y = Up(2)`, `Up` reading `x`). The same code path
   handles a primed expression in `Next`, `(Up(2))'`.
2. **A recursive action**, one that assigns primed variables
   (`Step(n) == IF n = 0 THEN x' = x + 1 ELSE Step(n - 1)`).

In both, the compiler inlines the operator instead of calling it — so that
`Init` sees the variables being assigned, and so that an action's primes stay
assignments — and inlining a recursive body unrolls it at compile time until
`inline` hits its depth limit of 64.

## Scope

In scope: the two positions above, including mutual recursion and variables
passed by name (`Step(n, x')`).

Out of scope:

- **A recursive action taking an operator or action argument.** Refused with a
  clear error instead of today's message.
- **Actions defined in a `LET`.** These fail even without recursion:
  `LET A(n) == x' = x + n IN A(1)` gives "x' is read before the action assigns
  it", because `is_action` does not look inside `LET` definitions. A separate
  fix; once it exists, `Act::Call` below makes a recursive `LET` action
  straightforward.

## Detecting recursion

`Compiler::is_recursive(n)`, memoized per name: definition `n` reaches itself
through the definitions its body names, following cfg substitutions
(`Op <- MCOp`).

- The `RECURSIVE` declarations cannot be used: the parser stores them as plain
  `Decl::Sym` entries, and they do not follow `INSTANCE` renaming.
- Shadowing cannot cause a false positive: TLA+ forbids a local binder that
  reuses a defined name.

Only operators for which `is_recursive` holds take the paths below; every
other operator compiles exactly as it does now.

## Part 1: recursion in `Init`

In `Init` mode, `Compiler::ident` and the `Ast::Apply` arm of `Compiler::expr`
inline any operator that reads state (`init && self.op_is_action(n, true)`).
For a recursive operator, emit instead `Expr::Call(op_index("n'"), args)`, with
the arguments compiled in `Init` mode.

The `n'` operator already exists, for instance substitutions:
`drain_pending` compiles `X'` as X's body with variables read from the next
state, which is what both `Init` and a primed expression need. Calls inside the
body resolve to the same `n'` operator.

No engine changes: all three engines already run `X'` operators, and a
generated `op_i` already takes `nx`.

## Part 2: recursive actions

### IR (`src/eval.rs`)

- `Act::Call(u32, Box<[Expr]>)`: an action operator called in a frame of its
  own.
- `Program.acts: Vec<ActOp>`, with
  `ActOp { name, nparams, frame, body: Act }`.

### Compiler (`src/compile.rs`)

In `Compiler::act`, the branch that inlines a definition sends a recursive
operator to a new `call_act` instead:

- The body is compiled once per key (name, `Init`/`Next` mode, which
  parameters `by_name` replaces with variables), in a fresh frame with the
  parameters in slots `0..n`.
- The index is registered before the body is compiled, so the operator's call
  to itself resolves to the same entry.
- Variables passed by name are handled as today: `by_name` runs first, and its
  substitution is part of the key.
- Operator parameters (`op(_)`) and action arguments are refused with a clear
  error: both are expanded in the caller's scope, which a separate frame cannot
  see.
- The depth limit in `inline` stays, as a guard only, and its message says what
  is actually unsupported.

### Interpreter (`Program::run`)

Arguments are evaluated into the slots above the caller's frame, as
`Program::call` does; then base and frame switch to the callee's and its body
runs with a wrapped continuation.

The caller's continuation runs while the callee's frame is still live: after
`k` returns, the callee may go on enumerating (`\E`, `\/`, `x' \in S`). So the
wrapper restores the caller's base but sets the caller's frame to the caller's
frame plus the callee's current frame size. A call the continuation makes then
opens above the callee's slots instead of overwriting them. When `k` returns,
the wrapper restores the callee's base and frame.

The callee's frame size is read when the continuation runs, not fixed at the
call, so nested recursive calls stack correctly.

### Closure engine (`src/closure.rs`)

The same logic. Action operators go in `Table` behind a `OnceLock`, as `ops`
do, because the closure for action operator i refers to entry i.

### Generated Rust (`src/codegen.rs`)

- Each action operator becomes
  `fn act_i<'a>(g: &G, cx: &mut Cx<'a>, k: K<'_, 'a>, a0: &Value, …) -> R<()>`.
- `Act::Call` evaluates its arguments, then calls
  `act_i(g, cx, &mut |cx: &mut Cx<'a>| -> R<()> { <k> Ok(()) }, &a0, …)?` —
  the closure pattern `Gen::share` already emits.
- There are no frames to protect: locals are Rust variables.
- The continuation is a `dyn FnMut`, so a function calling itself does not
  instantiate generics without end.
- `walk_program` also walks action operator bodies, appended last, so
  constant-pool indices for existing specs do not move.

### ENABLED and fairness

The interpreter and the closure engine run `Act` trees, so ENABLED handles
`Act::Call` with no further change. Generated code already hands ENABLED to the
interpreter, and liveness evaluates fairness through the interpreter.

## Performance and parallel processing

- **Specs without the new forms compile to the same IR.** `Program.acts` is
  empty, no `n'` calls appear, and generated Rust is byte-identical.
- **One deliberate change to specs that load today.** A recursive operator in
  action position can compile now when its recursive call lands in a guard:
  inlining stops after one level and the rest becomes an expression call (from
  reading the code: `IsEven(x)` as an `Init` conjunct). Under the new rule it
  becomes `Act::Call`. Same meaning; one frame per call.
- **Cost where the new forms are used:** one frame per recursion level, the
  same as an expression operator call. These specs are refused today, so there
  is no baseline to regress against.
- **Parallel processing is unaffected.** `Program`, `Closures` and the
  generated engine stay immutable and shared read-only (`Engine: Sync`).
  Per-worker state stays in `Bufs`, whose stack already grows on demand. No
  locks, atomics or shared mutable state are added.
- **Known limit:** very deep recursion uses the native Rust stack, as recursive
  expressions already do.

## Verification

1. **Byte-identical output.** `-codegen` on the gate and examples specs before
   and after; diff.
2. **Same results.** Re-run the validation sweep and
   `results/bench-examples-2026-10-07`; compare verdicts, state counts and
   timings.
3. **New test, `tests/recursive_actions.rs`, on all three engines:**
   - recursion in `Init`
   - a recursive action, and mutual recursion
   - variables passed by name
   - a recursive action under ENABLED in a fairness condition
   - a violation found through recursion

   Expected counts from TLC.
4. **Interpreter stress case.** A recursive action that enumerates several
   successors after a continuation that makes calls of its own: the frame
   overlap described under the interpreter.

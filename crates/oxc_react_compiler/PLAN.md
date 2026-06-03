# oxc_react_compiler — integration plan

Native oxc integration for the Rust port of React Compiler
([facebook/react#36173](https://github.com/facebook/react/pull/36173)).

**Status:** Compiles standalone against workspace oxc 0.134 (React core crates
pinned at rev `75f6a2b16b78`). The 0.121→0.134 drift turned out to be a single
change — `oxc_span::Atom` split into `oxc_str::{Ident, Str}` — confined to the
reverse converter; everything else compiled untouched. An end-to-end test
(`cargo test --manifest-path crates/oxc_react_compiler/Cargo.toml`) confirms a
component is fully memoized — `_c(n)` cache + `react/compiler-runtime` import —
through the real oxc→RC→oxc round trip. Next up: the `todo!()` guard, a full
fixture conformance harness, then wiring into the transform pipeline.

## Architecture decision

The React Compiler core is a whole-program, multi-pass IR compiler (AST → HIR →
CFG/SSA → ~30 passes → AST). Its public contract is deliberately narrow and
**frontend-agnostic**:

```
compile_program(file: react_compiler_ast::File, scope: ScopeInfo, opts: PluginOptions) -> CompileResult
```

Verified: the core crates (`react_compiler`, `react_compiler_ast`,
`react_compiler_hir`, `react_compiler_diagnostics`, …) depend only on
`serde`/`serde_json`/`indexmap` — **never on oxc**. The `oxc 0.121` pin lives
only in their example `react_compiler_oxc` glue crate.

So the chosen split:

- **Consume their core crates** as git dependencies (no oxc → no version clash).
- **Own the conversion layer** here, written against the live workspace oxc AST.
  These six modules are copied verbatim from `react_compiler_oxc` (targeting
  oxc 0.121) and must be ported to 0.134:
  - `prefilter.rs` — cheap `oxc_ast_visit::Visit` scan; skip files with no
    component/hook names (unless `compilationMode: "all"`).
  - `convert_ast.rs` — oxc `Program` → `react_compiler_ast::File` (~2.9k lines).
  - `convert_scope.rs` — oxc `Semantic`/`Scoping` → `ScopeInfo`.
  - `convert_ast_reverse.rs` — `File` → fresh oxc `Program` in a given allocator.
  - `apply_renames.rs` — patch references in uncompiled sibling functions.
  - `diagnostics.rs` — `CompileResult` → `oxc_diagnostics::OxcDiagnostic`.
- **Ignore their `react_compiler_oxc`** crate — reference only.

### Why excluded from the workspace (for now)

`crates/oxc_react_compiler` is in the root `Cargo.toml` `exclude` list. As a
member it would (a) force a `facebook/react` clone on every workspace cargo
invocation and (b) break repo-wide CI until the port compiles. Excluded, it
lives in-repo and builds standalone:

```
cargo build --manifest-path crates/oxc_react_compiler/Cargo.toml
```

Promote to a member (swap path deps for `{ workspace = true }`) once it compiles
and `react_compiler` is published to crates.io.

## Pipeline placement (decided)

React Compiler runs **first** — the earliest transform, on freshly-parsed
TS+JSX, before TS-strip / JSX-lowering / ES-downleveling:

```
parse → semantic → [react compiler] → TS-strip → decorator → plugins → JSX → es20xx → regexp
                   ^^^^^^^^^^^^^^^^^
                   pre-pass that brackets the traverse (NOT a Traverse callback)
```

It must see JSX + modern syntax + clean scope/spans (its `convert_scope` keys
nodes on `span.start` and assumes unique spans). It consumes TS natively
(`convert_ast.rs` handles `TSAsExpression`, `TSTypeAliasDeclaration`, type
params, …), so it can run ahead of TS-strip. Running it last is degenerate —
JSX/modern syntax would be gone and there'd be nothing to memoize.

## Work remaining (ordered)

1. **[deps] Resolve + pin.** ✅ Done. The three git deps resolve against
   `josephsavona/react`, pinned at rev `75f6a2b16b78`. React core crates have no
   oxc dependency, so they compile alongside workspace oxc 0.134 with no clash.
2. **[port] 0.121 → 0.134 API drift.** ✅ Done. The only drift: `oxc_span::Atom`
   was split into `oxc_str::{Ident, Str}` (identifier vs string newtypes). The
   reverse converter's `atom()` helper now returns an arena `&'a str` — which
   converts into both `Ident` and `Str` — so all ~53 builder call sites are
   unchanged; three direct struct-field assignments (regexp `text`, template
   `raw`/`cooked`) got an explicit `.into()`. `apply_renames` uses the same
   `StringBuilder` pattern. `convert_ast`/`convert_scope`/`prefilter` compiled
   verbatim. Re-port these spots on each upstream sync.
3. **[guard] Handle `todo!()` paths.** `convert_ast.rs` panics on exotic TS
   (`TSImportEqualsDeclaration`, `TSExportAssignment`, namespace exports,
   `V8IntrinsicExpression`, `PrivateInExpression`). Detect in the prefilter and
   skip the file (return unchanged) instead of panicking.
4. **[test] Round-trip + conformance.** 🟡 Started: a round-trip smoke test in
   `lib.rs` (memoizes a component; skips non-React code). Still TODO: port a
   slice of the PR's fixtures / `test-e2e.sh` to track output parity with the TS
   compiler.
5. **[perf] Kill the JSON boundary.** `compile_program` returns
   `ast: Option<Box<serde_json::value::RawValue>>` (serialized). Negotiate an
   in-process `File`/patch-returning entrypoint with Meta; until then, at least
   go `RawValue → File` directly (drop the `RawValue → Value → File` detour once
   the duplicate-`type`-key quirk is fixed upstream).
6. **[wire] Transform pipeline pre-pass.** Add `react_compiler` options to
   `TransformOptions`; in `Transformer::build_with_scoping`, before
   `traverse_mut_with_ctx`: prefilter → convert → `compile_program` → on
   `Some(ast)` convert back into the **same** `'a` allocator, apply renames,
   **rebuild `Scoping`** (RC invalidates it) → fall through to the existing
   traversal. `None` (no change) is free and preserves spans/comments.
7. **[expose] Surfaces.** `reactCompiler` flag in `napi/transform`; the much
   cheaper `lint()` (`no_emit`) path in oxlint for RC bailout diagnostics.
8. **[promote] Workspace member** once it compiles + core crates are published.

## Open questions / risks

- **Scope staleness** — RC invalidates oxc `Scoping`; downstream passes need a
  rebuild (or patch-localized refresh).
- **Spans / comments** — reverse conversion emits synthetic spans; the comment
  story is a source re-parse hack. Patches (from Meta) mostly sidestep this.
- **AST-shape churn** — Meta plan to move `react_compiler_ast` to arena alloc +
  `smol_str` and to return patches instead of a whole `Program`. Each bump =
  update these six modules. Owning them in-repo is what makes that tractable.
- **Clone cost** — the git dep clones a large repo; push for crates.io publish.

## Provenance

Conversion modules copied verbatim from
`facebook/react@rust-research:compiler/crates/react_compiler_oxc/src/` (MIT,
© Meta Platforms). Keep them close to upstream to ease re-syncing; record any
local edits here.

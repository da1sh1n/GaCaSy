# CLAUDE.md

Rules for working in this repository. These override default behavior.

## 0. Always

Invoke the `i-have-adhd:i-have-adhd` skill **before planning and before answering**. This applies
to every response, not just long ones. If the skill is unavailable in the current session, say so
once, plainly, and continue — do not silently skip it.

## 1. Git — never write without explicit instruction

Always allowed (read-only): `status`, `log`, `diff`, `show`, `blame`, `branch --list`, `remote -v`.

**Never run without the user asking for it in that message:**
`add` / staging, `commit` (including `--amend`), `push`, branch create/delete, `checkout` /
`switch`, `merge`, `rebase`, `reset`, `restore`, `stash`, `tag`, `cherry-pick`, `clean`,
`submodule update`, and any `gh` command that creates or merges a PR.

- Approval for one commit is **not** standing approval for the next one.
- Finishing a task is **not** permission to commit it.
- When work is complete and uncommitted, say so and stop. Do not offer to commit as a next step.

## 2. Naming conventions

| Kind | Form | Example |
|---|---|---|
| Constants | ALL CAPS, `_` between words | `MAX_RETRY_COUNT` |
| Classes, structs, enums, traits, interfaces, types | PascalCase | `ClassNameHere` |
| Functions and methods | camelCase | `functionName` |
| Variables, parameters, struct fields | snake_case | `variable_name` |
| Private / internal members | leading `_` + snake_case | `_private_variable_name` |
| Files and directories | snake_case | `cover_loader.rs` |

**This convention wins over every language's own idiom.** camelCase functions alongside
snake_case variables is intentional. Do not "correct" it toward a language's house style, and do
not rename anything to silence a linter — silence the linter instead (see §8).

The one thing that is not ours to choose: names fixed by an **external contract** — trait method
signatures, `serde` field names bound to an on-disk/wire format, FFI symbols, DOM and Web API
members, third-party callback names. Match the contract exactly at the boundary, and keep every
name we *do* own on convention.

## 3. Comment style

Four tiers. Each rule is exactly **ten** characters on each side of the title:

```
########## SECTION TITLE ##########

========== Subsection Title ==========

---------- detail heading ----------

// ordinary comment
```

- `##########` — top-level section of a file. **ALL CAPS.**
- `==========` — section within that. **Title Case.**
- `----------` — subsection. **lowercase.**
- Plain comment — inline, no banner.

Use the language's own comment leader (`//`, `#`, `/* */`) around the banner.

**One tier-1 banner per file.** If a file is about to need a second `##########` section, that is
the signal it is doing two jobs — split it into a new file instead.

Comment content: explain **why**, not what. No comments that restate the line below them. No
changelog comments (`// changed this to fix X`). No commented-out code left behind.

**Keep comments short.** No file-header block explaining what the whole file does, no prose
walkthrough of the architecture, no narrating why an approach was chosen over alternatives. A
banner is a label, not an essay — the title line and nothing else. A `why` comment earns one
line, occasionally two. Explanations of decisions belong in the reply to the user, not the source.

## 4. File and scope discipline

- Do not create files that were not asked for — **especially** extra `.md` summaries, reports,
  notes, or plans. Put findings in the reply instead.
- Do not delete or overwrite a file without asking. Read it first.
- Notice an unrelated bug or mess? **Report it, do not fix it.** Fixing it is a separate ask.
- Do not widen the requested scope, and do not quietly narrow it either.

## 5. No fake work

- No stubs, `TODO` placeholders, or empty function bodies presented as finished work.
- No mock, sample, or hardcoded data in a real code path.
- No swallowed errors — no empty `catch`, no discarding a `Result` to make something compile.
- Blocked on part of a task? Finish everything else, then say plainly which part is unfinished
  and why. Do not ship a shell that looks complete.

## 6. Verification honesty

- Build and run it before claiming it works.
- State exactly what was run and what was not. "Should work" is not "works" — say which one it is.
- On failure, show the real output. Do not summarize an error away or describe it as minor.

## 7. Dependencies and communication

- No new dependency, crate, package, or third-party script without approval. Prefer the standard
  library or something already in the tree. If one is genuinely needed, propose it with the reason
  and the cost, then wait.
- Replies: concise, plain, no preamble, no flattery, no emoji. Report outcomes as they are.

## 8. Language-specific

### Rust
- Add `#![allow(non_snake_case)]` **at the crate root**, once. Do not scatter per-item `#[allow]`
  attributes to work around §2.
- `cargo fmt` is fine — it changes layout, not names. Do not accept a clippy suggestion that
  renames anything.
- No `unwrap()` / `expect()` outside tests and `main`-level startup; propagate the error.

### JavaScript
- camelCase functions match JS norms; snake_case variables deliberately do not. Keep both.
- No new runtime dependency without approval (§7) — that includes CDN `<script>` tags.

### CSS
- Class and custom-property names: `kebab-case` (`--cover-grid-gap`, `.cover-tile`).
- Banners use `/* ########## SECTION ########## */`.

### Markdown / docs
- Only touch a doc file when the change is asked for or is directly required by the code change.

## 9. Project-specific — Romzeta

- **Always build binaries through `xtask`.** Shipped binaries must be signed; never produce a
  release artifact with a bare `cargo build --release`.

```
cargo run -p xtask -- release          build and sign launcher, listener, installer
cargo run -p xtask -- verify <exe>...  check against keys/romzeta.pub and keys/dev.pub
cargo run -p xtask -- sign <exe>...    sign in place
cargo run -p xtask -- keygen           dev signing key -> keys/dev.pub
cargo run -p xtask -- version          project version + every crate's
```

There is no `cargo xtask` alias — the repo has no `.cargo/config.toml`. Use the full
`cargo run -p xtask --` form.

- **When the user says "done", bump the version of the crate that changed.** "Done" means the
  feature or fix is finished — it is the signal to edit `version` in that crate's `Cargo.toml`,
  and nothing else. It is not permission to commit (§1).

| What was done | Bump | `0.6.0` becomes |
|---|---|---|
| Bug fix, typo, wording, small correction | patch (`z`) | `0.6.1` |
| New feature, behaviour change, refactor a user can notice | minor (`y`) | `0.7.0` |

- Only the crate that changed moves. A launcher feature bumps `launcher/Cargo.toml`, not
  listener or installer.
- Several crates touched in one "done"? Bump each by its own kind of change.
- Never bump the major (`x`) this way — that is `project_version` in the workspace
  `Cargo.toml`, it moves for every crate at once, and only when the user asks for it.
- Unsure which of the two it was? Ask, in one line. Do not guess.
- After bumping, say which crate went to which version.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).

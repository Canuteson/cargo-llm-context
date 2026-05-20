# cargo-llm-context

A Cargo subcommand that generates condensed, ownership-annotated API context files for LLM
agents working in Rust codebases.

```
cargo llm-context [OPTIONS] [PATH]
```

---

## The Problem

LLM agents write good Rust at small scale. At large scale they lose track of the ownership
graph — who owns what, what borrows from where, which types are cheap to clone and which
are not. The result is borrow checker failures, `.clone()` spam, and unnecessary `Arc<Mutex<>>`
wrapping.

Standard rustdoc is structured for human browsing, not agent consumption. An agent about to
edit `my_crate::network::session` doesn't need the full rustdoc tree — it needs a focused
~800-token file covering that module's types, their ownership semantics, and the key method
signatures. That is what this tool produces.

---

## Output

For a crate with modules `ecs::world`, `ecs::query`, and `event`:

```
ai-context/
├── _index.md          # Navigation: one line per type, which file to load
├── ecs-world.md       # ~800 tokens: World, Entity, ownership contracts
├── ecs-query.md       # ~800 tokens: Query, QueryIter, borrow relationships
└── event.md           # ~800 tokens: EventBus, EventReader
```

**The index** is loaded first. It tells the agent which module file to load next, based on
what it is about to edit.

**Per-module files** are loaded before editing. Each covers one `mod` boundary and stays
within a configurable token budget.

**The merged file** (`--merge`) concatenates everything for callers that want the full
picture in a single context load.

---

## Ownership Annotation

For each public type, the tool emits:

1. **Inferred ownership class** from field structure:
   - `owns` — contains `Vec`, `Box`, `String`, `HashMap`, etc.
   - `shared (Arc/Rc)` — wraps shared ownership
   - `borrows` — has lifetime parameters
   - `handle (Copy)` — Copy type wrapping an ID or index
   - `opaque` — cannot be determined structurally

2. **Explicit ownership doc** — if the type or its parent module has a doc comment with an
   `## Ownership` section (see [doc conventions](#doc-conventions)), that text is shown
   verbatim and takes precedence over the inferred label.

Example output for a module file:

````markdown
# `ecs::world`

## Ownership
This module owns all component data for all live entities. Callers receive `&mut World`
and must not retain borrows across `World::flush`.

## `World` — owns

```rust
pub struct World
    pub fn new() -> World
    pub fn spawn(&mut self, bundle: impl Bundle) -> Entity
    pub fn despawn(&mut self, entity: Entity) -> bool
    pub fn flush(&mut self)
```

_Derives: Debug_

_contains Vec/Box/String or similar — sole owner of heap data; cloning is a deep copy_

---

## `Entity` — handle (Copy)

```rust
pub struct Entity(pub(crate) EntityId)
    pub fn id(self) -> EntityId
```

_Derives: Debug, Clone, Copy, PartialEq, Eq, Hash_

_Copy type — cloning is trivial and does not duplicate any state_
````

---

## Installation

```sh
cargo install cargo-llm-context
```

Or build from source:

```sh
git clone https://github.com/Canuteson/cargo-llm-context
cd cargo-llm-context
cargo install --path .
```

---

## Usage

```sh
# Single crate (run from crate root)
cargo llm-context

# Specific crate in a workspace
cargo llm-context --krate hearth-core

# From a different directory
cargo llm-context /path/to/my-crate

# Also write a merged single-file output
cargo llm-context --merge

# Adjust token budget per module file (default: 800)
cargo llm-context --token-budget 1200

# Custom output directory
cargo llm-context --output-dir ./docs/ai-context
```

After running, feed the agent context in two steps:

1. Load `_index.md` first — the agent identifies which module it needs.
2. Load the relevant `<module>.md` before editing that module.

### Committing the output

The `ai-context/` directory is **documentation, not a build artifact** — commit it alongside
your source code. Unlike `target/`, it should be checked in and reviewed in PRs. When the
public API changes, re-run `cargo llm-context` and commit the updated files.

Add to `.gitignore`:
```
target/
```

Do **not** add `ai-context/` to `.gitignore`.

---

## Doc Conventions

The tool extracts richer output from codebases that follow these conventions. These are
the same conventions used in the [Hearth engine CLAUDE.md](../hearth-engine/CLAUDE.md).

### Module-level ownership doc

Add an `## Ownership` section to your module's doc comment:

```rust
//! Manages the entity component storage layer.
//!
//! ## Ownership
//! This module owns all component data for all live entities. The `World` type
//! is the sole owner of `Storage`; nothing borrows component data across frame
//! boundaries. Callers receive `&mut World` and submit commands via `World::spawn`.
```

### Type-level ownership doc

Add the same section to individual types that have non-obvious semantics:

```rust
/// Handle to a live entity. Generation field prevents use-after-free of recycled slots.
///
/// ## Ownership
/// This is a lightweight Copy handle. Cloning it does not clone the entity's components.
/// The entity's data lives in `World` and is freed by `World::despawn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Entity(pub(crate) EntityId);
```

Types without explicit `## Ownership` sections get inferred annotations from field types.

---

## Architecture

```
src/
├── main.rs       CLI entry point, orchestration
├── types.rs      OwnershipClass, ApiItem, Module — shared data types
├── extract.rs    Follows mod declarations from lib.rs, parses with syn,
│                 collects public items and attaches impl methods
├── ownership.rs  Infers OwnershipClass from field types and generics
├── condense.rs   Renders Module → bounded markdown, ranks items by importance
└── index.rs      Renders Vec<Module> → _index.md navigation file
```

**Data source:** `syn` (source parsing, stable Rust). No compilation step required. A large
crate with 50 source files is processed in under a second.

**Token estimation:** 1 token ≈ 4 characters. The `--token-budget` flag controls the per-module
limit; items are ranked (types before functions before aliases) and truncated at the budget.

**No nightly required.** No LLM API key required. Works on any Rust crate you have source
access to.

---

## Roadmap

- [ ] `--backend rustdoc` flag: use `cargo rustdoc --output-format json` for full type
  resolution when nightly is available
- [x] Re-export tracking: follow `pub use` statements to include re-exported items
- [ ] Trait impl annotation: mark types that implement `Send + Sync`, `Clone`, `Default`
- [ ] `--watch` mode: re-generate on file changes during active development
- [ ] Integration with `ARCHITECTURE.toml` to annotate cross-crate ownership relationships

---

## Why not rustdoc JSON?

`rustdoc --output-format json` requires `-Z unstable-options` (nightly toolchain) and a
full compilation pass. That combination is a significant adoption barrier — many teams
don't run nightly, and requiring a successful build blocks use on in-progress codebases.

`syn` parsing is structural, not semantic: it cannot follow type aliases or resolve generic
bounds through trait impls. In practice, the ownership patterns that matter most (`Vec`,
`Arc`, lifetime parameters, `Copy` derives) are directly visible in field declarations and
structural parsing captures them correctly. The `## Ownership` doc convention bridges the
gap for cases where inference is insufficient.

A `--backend rustdoc` flag is planned for teams that have nightly available and want
complete type resolution.

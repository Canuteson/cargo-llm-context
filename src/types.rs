/// How a type relates to the data it contains.
/// Inferred from field types and generics; overridden by `## Ownership` doc sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipClass {
    /// Contains Vec, Box, String, HashMap, etc. — sole owner of heap data.
    Owns,
    /// Contains Arc or Rc — shared ownership, cheap to clone the handle.
    Shared,
    /// Has lifetime parameters — borrows from a caller or arena.
    Borrows,
    /// Copy type wrapping an index or ID — cloning is trivial, not a copy of state.
    Handle,
    /// Cannot be determined from structure alone.
    Opaque,
}

impl OwnershipClass {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Owns => "owns",
            Self::Shared => "shared (Arc/Rc)",
            Self::Borrows => "borrows",
            Self::Handle => "handle (Copy)",
            Self::Opaque => "opaque",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ItemKind {
    Struct,
    Enum,
    Trait,
    Function,
    TypeAlias,
}

#[derive(Debug, Clone)]
pub struct MethodSig {
    /// Full `pub fn foo(&self, ...) -> Bar` string.
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct ApiItem {
    pub name: String,
    pub kind: ItemKind,
    pub ownership: OwnershipClass,
    /// Extracted text of the `## Ownership` doc section, if present.
    pub ownership_doc: Option<String>,
    /// Traits listed in `#[derive(...)]`.
    pub derives: Vec<String>,
    /// `pub struct Foo<T>` / `pub fn bar(x: X) -> Y` signature string.
    pub signature: String,
    /// Methods from `impl` blocks targeting this type.
    pub methods: Vec<MethodSig>,
}

/// A single `pub use` entry, flattened from a potentially grouped use tree.
#[derive(Debug, Clone)]
pub struct Reexport {
    /// Full dotted path, e.g. `"glam::Vec3"` or `"crate::ecs::entity::Entity"`.
    pub path: String,
    /// Rename target if `pub use foo as Bar`.
    pub alias: Option<String>,
    /// True for `pub use foo::*`.
    pub is_glob: bool,
    /// True if the path starts with `crate::`, `self::`, or `super::`.
    pub is_internal: bool,
}

impl Reexport {
    /// The name the item is exposed as at this use site.
    pub fn exported_name(&self) -> &str {
        if let Some(alias) = &self.alias {
            return alias.as_str();
        }
        if self.is_glob {
            return "*";
        }
        // Last segment of the path.
        self.path.rsplit("::").next().unwrap_or(&self.path)
    }
}

#[derive(Debug, Clone)]
pub struct Module {
    /// Full `::` path relative to crate root, e.g. `"ecs::world"`.
    pub path: String,
    /// Text of `## Ownership` from the module-level doc comment, if present.
    pub ownership_doc: Option<String>,
    pub items: Vec<ApiItem>,
    /// All `pub use` statements in this module, flattened.
    pub reexports: Vec<Reexport>,
}

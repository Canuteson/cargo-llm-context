use crate::types::{ApiItem, ItemKind, MethodSig, Module, OwnershipClass, Reexport};
use anyhow::{Context, Result};
use quote::ToTokens;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::visit::Visit;

pub fn extract_crate(root: &Path, crate_name: Option<&str>) -> Result<Vec<Module>> {
    let manifest = root.join("Cargo.toml");
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest)
        .exec()
        .context("cargo metadata failed — is this a Cargo project?")?;

    let package = if let Some(name) = crate_name {
        metadata.packages.iter().find(|p| p.name == name)
    } else {
        metadata.root_package()
    }
    .with_context(|| "could not find package in workspace")?;

    let lib_target = package
        .targets
        .iter()
        .find(|t| t.kind.iter().any(|k| k == "lib" || k == "proc-macro"))
        .context("no lib target found — is this a library crate?")?;

    let mut modules = collect_from_file(lib_target.src_path.as_std_path(), &[])?;
    mark_internal_reexports(&mut modules);
    Ok(modules)
}

/// Re-exports using bare module-relative paths (e.g. `entity::Entity`) are internal but
/// don't carry the `crate::/self::` prefix. After the full module tree is collected, we
/// check if a re-export's first path segment matches a known module name and promote it.
fn mark_internal_reexports(modules: &mut Vec<Module>) {
    let known_segments: HashSet<String> = modules
        .iter()
        .flat_map(|m| m.path.split("::").map(str::to_string))
        .collect();

    for module in modules.iter_mut() {
        for reexport in module.reexports.iter_mut() {
            if !reexport.is_internal {
                let first = reexport.path.split("::").next().unwrap_or("");
                if known_segments.contains(first) {
                    reexport.is_internal = true;
                }
            }
        }
    }
}

/// Recursively collect public items from a source file and its `mod` children.
fn collect_from_file(path: &Path, module_path: &[String]) -> Result<Vec<Module>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;

    let file = syn::parse_file(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;

    let mut result = Vec::new();
    let mut top_items: Vec<ApiItem> = Vec::new();
    let mut impl_map: HashMap<String, Vec<MethodSig>> = HashMap::new();
    let mut reexports: Vec<Reexport> = Vec::new();
    let module_doc = extract_ownership_section_from_file_attrs(&file.attrs);

    for item in &file.items {
        match item {
            syn::Item::Struct(s) if is_public(&s.vis) => {
                top_items.push(collect_struct(s));
            }
            syn::Item::Enum(e) if is_public(&e.vis) => {
                top_items.push(collect_enum(e));
            }
            syn::Item::Fn(f) if is_public(&f.vis) => {
                top_items.push(collect_fn(f));
            }
            syn::Item::Type(t) if is_public(&t.vis) => {
                top_items.push(collect_type_alias(t));
            }
            syn::Item::Trait(t) if is_public(&t.vis) => {
                top_items.push(collect_trait(t));
            }
            syn::Item::Use(u) if is_public(&u.vis) => {
                reexports.extend(collect_reexports(u));
            }
            syn::Item::Impl(imp) if imp.trait_.is_none() => {
                // Inherent impl — collect methods and attach to their type later.
                if let syn::Type::Path(tp) = imp.self_ty.as_ref() {
                    if let Some(name) = tp.path.get_ident() {
                        let methods = collect_impl_methods(imp);
                        impl_map.entry(name.to_string()).or_default().extend(methods);
                    }
                }
            }
            syn::Item::Mod(m) if is_public(&m.vis) => {
                let mod_name = m.ident.to_string();
                let mut child_path = module_path.to_vec();
                child_path.push(mod_name.clone());

                if let Some((_, content)) = &m.content {
                    // Inline module — synthesize a pseudo-file and recurse.
                    let pseudo_file = syn::File {
                        shebang: None,
                        attrs: m.attrs.clone(),
                        items: content.clone(),
                    };
                    result.extend(collect_from_syn_file(&pseudo_file, &child_path));
                } else {
                    // File module — resolve path and recurse.
                    let current_dir = path.parent().unwrap_or(Path::new("."));
                    let candidates: [PathBuf; 2] = [
                        current_dir.join(format!("{mod_name}.rs")),
                        current_dir.join(&mod_name).join("mod.rs"),
                    ];
                    let found = candidates.iter().find(|p| p.exists());
                    match found {
                        Some(child_path_file) => {
                            match collect_from_file(child_path_file, &child_path) {
                                Ok(modules) => result.extend(modules),
                                Err(e) => eprintln!("warning: {e}"),
                            }
                        }
                        None => {
                            eprintln!("warning: could not find file for `mod {mod_name}`");
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Attach collected methods to their types.
    for item in &mut top_items {
        if let Some(methods) = impl_map.remove(&item.name) {
            item.methods = methods;
        }
    }

    if !top_items.is_empty() || !reexports.is_empty() || module_doc.is_some() {
        let path_str = module_path.join("::");
        result.push(Module {
            name: module_path.last().cloned().unwrap_or_else(|| "lib".into()),
            path: if path_str.is_empty() { "lib".into() } else { path_str },
            ownership_doc: module_doc,
            items: top_items,
            reexports,
        });
    }

    Ok(result)
}

/// Same as `collect_from_file` but from an already-parsed `syn::File` (for inline modules).
fn collect_from_syn_file(file: &syn::File, module_path: &[String]) -> Vec<Module> {
    let mut items: Vec<ApiItem> = Vec::new();
    let mut impl_map: HashMap<String, Vec<MethodSig>> = HashMap::new();
    let mut reexports: Vec<Reexport> = Vec::new();
    let module_doc = extract_ownership_section_from_file_attrs(&file.attrs);

    for item in &file.items {
        match item {
            syn::Item::Struct(s) if is_public(&s.vis) => items.push(collect_struct(s)),
            syn::Item::Enum(e) if is_public(&e.vis) => items.push(collect_enum(e)),
            syn::Item::Fn(f) if is_public(&f.vis) => items.push(collect_fn(f)),
            syn::Item::Type(t) if is_public(&t.vis) => items.push(collect_type_alias(t)),
            syn::Item::Trait(t) if is_public(&t.vis) => items.push(collect_trait(t)),
            syn::Item::Use(u) if is_public(&u.vis) => reexports.extend(collect_reexports(u)),
            syn::Item::Impl(imp) if imp.trait_.is_none() => {
                if let syn::Type::Path(tp) = imp.self_ty.as_ref() {
                    if let Some(name) = tp.path.get_ident() {
                        impl_map
                            .entry(name.to_string())
                            .or_default()
                            .extend(collect_impl_methods(imp));
                    }
                }
            }
            _ => {}
        }
    }

    for item in &mut items {
        if let Some(methods) = impl_map.remove(&item.name) {
            item.methods = methods;
        }
    }

    if items.is_empty() && reexports.is_empty() && module_doc.is_none() {
        return Vec::new();
    }

    vec![Module {
        name: module_path.last().cloned().unwrap_or_else(|| "lib".into()),
        path: module_path.join("::"),
        ownership_doc: module_doc,
        items,
        reexports,
    }]
}

// ── Item collectors ──────────────────────────────────────────────────────────

fn collect_struct(s: &syn::ItemStruct) -> ApiItem {
    let derives = extract_derives(&s.attrs);
    let ownership = crate::ownership::infer(&s.fields, &s.generics, &derives);
    let ownership_doc = extract_ownership_section(&s.attrs);
    let sig = format!(
        "pub struct {}{}",
        s.ident,
        s.generics.to_token_stream()
    );
    ApiItem {
        name: s.ident.to_string(),
        kind: ItemKind::Struct,
        ownership,
        ownership_doc,
        derives,
        signature: sig,
        methods: Vec::new(),
    }
}

fn collect_enum(e: &syn::ItemEnum) -> ApiItem {
    let derives = extract_derives(&e.attrs);
    // Enums: check if all variants are unit — treat as Handle-like if Copy
    let ownership = if derives.contains(&"Copy".to_string()) {
        OwnershipClass::Handle
    } else {
        OwnershipClass::Opaque
    };
    let sig = format!("pub enum {}{}", e.ident, e.generics.to_token_stream());
    ApiItem {
        name: e.ident.to_string(),
        kind: ItemKind::Enum,
        ownership,
        ownership_doc: extract_ownership_section(&e.attrs),
        derives,
        signature: sig,
        methods: Vec::new(),
    }
}

fn collect_fn(f: &syn::ItemFn) -> ApiItem {
    let sig = format!("pub {}", f.sig.to_token_stream());
    ApiItem {
        name: f.sig.ident.to_string(),
        kind: ItemKind::Function,
        ownership: OwnershipClass::Opaque,
        ownership_doc: extract_ownership_section(&f.attrs),
        derives: Vec::new(),
        signature: sig,
        methods: Vec::new(),
    }
}

fn collect_type_alias(t: &syn::ItemType) -> ApiItem {
    let sig = format!(
        "pub type {}{} = {}",
        t.ident,
        t.generics.to_token_stream(),
        t.ty.to_token_stream()
    );
    ApiItem {
        name: t.ident.to_string(),
        kind: ItemKind::TypeAlias,
        ownership: OwnershipClass::Opaque,
        ownership_doc: None,
        derives: Vec::new(),
        signature: sig,
        methods: Vec::new(),
    }
}

fn collect_trait(t: &syn::ItemTrait) -> ApiItem {
    let sig = format!("pub trait {}{}", t.ident, t.generics.to_token_stream());
    ApiItem {
        name: t.ident.to_string(),
        kind: ItemKind::Trait,
        ownership: OwnershipClass::Opaque,
        ownership_doc: extract_ownership_section(&t.attrs),
        derives: Vec::new(),
        signature: sig,
        methods: Vec::new(),
    }
}

fn collect_impl_methods(imp: &syn::ItemImpl) -> Vec<MethodSig> {
    imp.items
        .iter()
        .filter_map(|item| {
            if let syn::ImplItem::Fn(method) = item {
                if is_public(&method.vis) {
                    return Some(MethodSig {
                        name: method.sig.ident.to_string(),
                        signature: format!("pub {}", method.sig.to_token_stream()),
                    });
                }
            }
            None
        })
        .collect()
}

// ── Re-export helpers ────────────────────────────────────────────────────────

fn collect_reexports(u: &syn::ItemUse) -> Vec<Reexport> {
    let leading = if u.leading_colon.is_some() { "::" } else { "" };
    flatten_use_tree(&u.tree, leading)
        .into_iter()
        .map(|(path, alias, is_glob)| {
            let is_internal = path.starts_with("crate::")
                || path.starts_with("self::")
                || path.starts_with("super::");
            Reexport { path, alias, is_glob, is_internal }
        })
        .collect()
}

/// Recursively flatten a `UseTree` into `(canonical_path, alias, is_glob)` tuples.
fn flatten_use_tree(
    tree: &syn::UseTree,
    prefix: &str,
) -> Vec<(String, Option<String>, bool)> {
    match tree {
        syn::UseTree::Path(p) => {
            let next = if prefix.is_empty() {
                p.ident.to_string()
            } else {
                format!("{prefix}::{}", p.ident)
            };
            flatten_use_tree(&p.tree, &next)
        }
        syn::UseTree::Name(n) => {
            let path = if prefix.is_empty() {
                n.ident.to_string()
            } else {
                format!("{prefix}::{}", n.ident)
            };
            vec![(path, None, false)]
        }
        syn::UseTree::Rename(r) => {
            let path = if prefix.is_empty() {
                r.ident.to_string()
            } else {
                format!("{prefix}::{}", r.ident)
            };
            vec![(path, Some(r.rename.to_string()), false)]
        }
        syn::UseTree::Glob(_) => {
            let path = if prefix.is_empty() {
                "*".to_string()
            } else {
                format!("{prefix}::*")
            };
            vec![(path, None, true)]
        }
        syn::UseTree::Group(g) => g
            .items
            .iter()
            .flat_map(|item| flatten_use_tree(item, prefix))
            .collect(),
    }
}

// ── Doc comment helpers ──────────────────────────────────────────────────────

fn doc_string(attrs: &[syn::Attribute]) -> String {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .filter_map(|a| {
            if let syn::Meta::NameValue(nv) = &a.meta {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = &nv.value
                {
                    return Some(s.value());
                }
            }
            None
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract the `## Ownership` section from item-level doc attributes.
fn extract_ownership_section(attrs: &[syn::Attribute]) -> Option<String> {
    ownership_section_from_str(&doc_string(attrs))
}

/// Extract the `## Ownership` section from file-level inner doc attributes (`#![doc = ...]`).
fn extract_ownership_section_from_file_attrs(attrs: &[syn::Attribute]) -> Option<String> {
    let doc = attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .filter_map(|a| {
            if let syn::Meta::NameValue(nv) = &a.meta {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = &nv.value
                {
                    return Some(s.value());
                }
            }
            None
        })
        .collect::<Vec<_>>()
        .join("\n");
    ownership_section_from_str(&doc)
}

fn ownership_section_from_str(doc: &str) -> Option<String> {
    let start = doc.find("## Ownership")?;
    let section = &doc[start..];
    // End at the next `## ` heading or end of string.
    let end = section[2..]
        .find("\n## ")
        .map(|i| i + 2)
        .unwrap_or(section.len());
    Some(section[..end].trim().to_string())
}

fn extract_derives(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("derive"))
        .flat_map(|a| {
            let mut out = Vec::new();
            if let syn::Meta::List(list) = &a.meta {
                if let Ok(paths) = list.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                ) {
                    for p in paths {
                        if let Some(ident) = p.get_ident() {
                            out.push(ident.to_string());
                        }
                    }
                }
            }
            out
        })
        .collect()
}

fn is_public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

// ── Visitor for impl blocks (used to attach methods post-hoc) ────────────────

struct ImplVisitor<'a> {
    map: &'a mut HashMap<String, Vec<MethodSig>>,
}

impl<'ast> Visit<'ast> for ImplVisitor<'_> {
    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if node.trait_.is_some() {
            return;
        }
        if let syn::Type::Path(tp) = node.self_ty.as_ref() {
            if let Some(name) = tp.path.get_ident() {
                self.map
                    .entry(name.to_string())
                    .or_default()
                    .extend(collect_impl_methods(node));
            }
        }
    }
}

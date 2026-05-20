use crate::types::{ApiItem, ItemKind, Module, OwnershipClass, Reexport};
use std::collections::BTreeMap;

/// Approximate token count: 1 token ≈ 4 characters.
fn token_estimate(s: &str) -> usize {
    s.len() / 4
}

/// Render a module to markdown, staying within `token_budget` tokens.
/// Items are ranked by importance: types first, then functions, then aliases.
pub fn render_module(module: &Module, token_budget: usize) -> String {
    let mut out = String::new();

    // Header
    out.push_str(&format!("# `{}`\n\n", module.path));

    // Module ownership doc (from `## Ownership` in module-level doc comment)
    if let Some(doc) = &module.ownership_doc {
        out.push_str(doc);
        out.push_str("\n\n");
    }

    let mut budget_remaining = token_budget.saturating_sub(token_estimate(&out));

    // Re-exports block — rendered before items so the agent knows what's in scope.
    if !module.reexports.is_empty() {
        let block = render_reexports(&module.reexports);
        let cost = token_estimate(&block);
        if cost <= budget_remaining {
            out.push_str(&block);
            budget_remaining = budget_remaining.saturating_sub(cost);
        }
    }

    // Rank items: structs/enums/traits before functions and aliases.
    let mut ranked: Vec<&ApiItem> = module.items.iter().collect();
    ranked.sort_by_key(|item| match item.kind {
        ItemKind::Struct | ItemKind::Enum | ItemKind::Trait => 0,
        ItemKind::Function => 1,
        ItemKind::TypeAlias => 2,
    });

    for item in ranked {
        let block = render_item(item);
        let cost = token_estimate(&block);
        if cost > budget_remaining {
            out.push_str(&format!(
                "\n_... {} more items omitted (token budget reached)_\n",
                module.items.len()
                    - module
                        .items
                        .iter()
                        .position(|i| std::ptr::eq(i, item))
                        .unwrap_or(0)
            ));
            break;
        }
        out.push_str(&block);
        budget_remaining = budget_remaining.saturating_sub(cost);
    }

    out
}

fn render_item(item: &ApiItem) -> String {
    let mut out = String::new();

    // Section header with ownership class
    out.push_str(&format!(
        "## `{}` — {}\n\n",
        item.name,
        item.ownership.label()
    ));

    // Signature
    out.push_str("```rust\n");
    out.push_str(&item.signature);
    out.push('\n');

    // Methods (first 8 to stay bounded)
    for method in item.methods.iter().take(8) {
        out.push_str(&format!("    {}\n", method.signature));
    }
    if item.methods.len() > 8 {
        out.push_str(&format!("    // ... {} more methods\n", item.methods.len() - 8));
    }
    out.push_str("```\n\n");

    // Derives
    if !item.derives.is_empty() {
        out.push_str(&format!(
            "_Derives: {}_\n\n",
            item.derives.join(", ")
        ));
    }

    // Ownership note: prefer explicit doc section, fall back to inferred label
    match &item.ownership_doc {
        Some(doc) => {
            out.push_str(doc);
            out.push_str("\n\n");
        }
        None if item.ownership != OwnershipClass::Opaque => {
            out.push_str(&format!(
                "_Ownership inferred from structure: {}_\n\n",
                ownership_explanation(&item.ownership)
            ));
        }
        None => {}
    }

    out.push_str("---\n\n");
    out
}

/// Render the re-exports section, grouped by source crate/module.
fn render_reexports(reexports: &[Reexport]) -> String {
    // Group by the source prefix (everything except the final name segment).
    // External: "glam::Vec3" → source "glam"
    // Internal: "crate::ecs::entity::Entity" → source "crate::ecs::entity (internal)"
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for r in reexports {
        let source = source_label(r);
        let name = if let Some(alias) = &r.alias {
            format!("{} as {alias}", r.exported_name())
        } else {
            r.exported_name().to_string()
        };
        groups.entry(source).or_default().push(format!("`{name}`"));
    }

    let mut out = String::from("## Re-exports\n\n");
    for (source, names) in &groups {
        out.push_str(&format!("**{}:** {}\n\n", source, names.join(", ")));
    }
    out
}

/// Derive a human-readable source label for grouping, e.g. `"glam"` or `"crate::ecs (internal)"`.
fn source_label(r: &Reexport) -> String {
    if r.is_glob {
        // "glam::*" → "glam (glob)"
        let prefix = r.path.trim_end_matches("::*").trim_end_matches("::*");
        let base = first_segment(prefix);
        if r.is_internal {
            return format!("{prefix} (internal, glob)");
        }
        return format!("{base} (glob)");
    }

    if r.is_internal {
        // "crate::ecs::entity::Entity" → "crate::ecs::entity (internal)"
        let prefix = strip_last_segment(&r.path);
        return format!("{prefix} (internal)");
    }

    // "glam::Vec3" → "glam"
    first_segment(&r.path).to_string()
}

fn first_segment(path: &str) -> &str {
    path.split("::").next().unwrap_or(path)
}

fn strip_last_segment(path: &str) -> &str {
    match path.rfind("::") {
        Some(i) => &path[..i],
        None => path,
    }
}

fn ownership_explanation(class: &OwnershipClass) -> &'static str {
    match class {
        OwnershipClass::Owns => {
            "contains Vec/Box/String or similar — sole owner of heap data; cloning is a deep copy"
        }
        OwnershipClass::Shared => {
            "contains Arc or Rc — cloning the handle is cheap; underlying data is shared"
        }
        OwnershipClass::Borrows => {
            "has lifetime parameters — borrows data from a caller or arena; cannot outlive its source"
        }
        OwnershipClass::Handle => {
            "Copy type — cloning is trivial and does not duplicate any state"
        }
        OwnershipClass::Opaque => "",
    }
}

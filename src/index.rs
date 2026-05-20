use crate::types::{ItemKind, Module};

/// Render a lightweight navigation index: one line per item, grouped by module.
/// Intended to be loaded first so an agent can identify which module file to load next.
pub fn render_index(modules: &[Module]) -> String {
    let mut out = String::new();
    out.push_str("# API Index\n\n");
    out.push_str("Load the per-module file before editing that module.\n");
    out.push_str("File names follow the pattern `<module-path>.md` with `::` replaced by `-`.\n\n");

    for module in modules {
        out.push_str(&format!("## `{}`\n\n", module.path));

        if let Some(doc) = &module.ownership_doc {
            // Show just the first line of the ownership summary
            let first_line = doc.lines().nth(1).unwrap_or("").trim();
            if !first_line.is_empty() {
                out.push_str(&format!("> {first_line}\n\n"));
            }
        }

        let file = module.path.replace("::", "-") + ".md";
        out.push_str(&format!("Context file: `{file}`\n\n"));

        // Types
        let types: Vec<_> = module
            .items
            .iter()
            .filter(|i| {
                matches!(
                    i.kind,
                    ItemKind::Struct | ItemKind::Enum | ItemKind::Trait | ItemKind::TypeAlias
                )
            })
            .collect();

        if !types.is_empty() {
            out.push_str("| Type | Ownership | Methods |\n");
            out.push_str("|---|---|---|\n");
            for item in &types {
                out.push_str(&format!(
                    "| `{}` | {} | {} |\n",
                    item.name,
                    item.ownership.label(),
                    item.methods.len()
                ));
            }
            out.push('\n');
        }

        // Standalone functions
        let fns: Vec<_> = module
            .items
            .iter()
            .filter(|i| matches!(i.kind, ItemKind::Function))
            .collect();

        if !fns.is_empty() {
            out.push_str("Functions: ");
            out.push_str(
                &fns.iter()
                    .map(|f| format!("`{}`", f.name))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            out.push_str("\n\n");
        }

        // Re-exports summary: one line per source, names elided after 6
        if !module.reexports.is_empty() {
            let (internal, external): (Vec<_>, Vec<_>) =
                module.reexports.iter().partition(|r| r.is_internal);

            if !external.is_empty() {
                let names: Vec<_> = external
                    .iter()
                    .map(|r| format!("`{}`", r.exported_name()))
                    .collect();
                let display = if names.len() > 6 {
                    format!("{} … ({} total)", names[..6].join(", "), names.len())
                } else {
                    names.join(", ")
                };
                out.push_str(&format!("Re-exports (external): {display}\n\n"));
            }

            if !internal.is_empty() {
                let names: Vec<_> = internal
                    .iter()
                    .map(|r| format!("`{}`", r.exported_name()))
                    .collect();
                let display = if names.len() > 6 {
                    format!("{} … ({} total)", names[..6].join(", "), names.len())
                } else {
                    names.join(", ")
                };
                out.push_str(&format!("Re-exports (internal): {display}\n\n"));
            }
        }
    }

    out
}

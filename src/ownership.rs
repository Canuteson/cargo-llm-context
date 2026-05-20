use crate::types::OwnershipClass;

/// Infer ownership class from a struct's fields and generic parameters.
/// `derives` should be the list of traits from `#[derive(...)]` on the type.
pub fn infer(
    fields: &syn::Fields,
    generics: &syn::Generics,
    derives: &[String],
) -> OwnershipClass {
    // A Copy type wrapping primitive fields is a handle, not an owner.
    if derives.contains(&"Copy".to_string()) {
        return OwnershipClass::Handle;
    }

    // Lifetime parameters mean the type borrows from somewhere.
    if generics.lifetimes().next().is_some() {
        return OwnershipClass::Borrows;
    }

    // Walk fields looking for ownership-indicating types.
    for field in fields.iter() {
        if let Some(class) = from_type(&field.ty) {
            return class;
        }
    }

    OwnershipClass::Opaque
}

/// Recursively inspect a type to find ownership indicators.
/// Returns the first match found; callers should prefer the outermost.
fn from_type(ty: &syn::Type) -> Option<OwnershipClass> {
    match ty {
        syn::Type::Path(tp) => from_path(&tp.path),
        syn::Type::Reference(_) => Some(OwnershipClass::Borrows),
        syn::Type::Tuple(t) => t.elems.iter().find_map(from_type),
        _ => None,
    }
}

fn from_path(path: &syn::Path) -> Option<OwnershipClass> {
    let last = path.segments.last()?;
    match last.ident.to_string().as_str() {
        // Sole ownership of heap data
        "Vec" | "Box" | "String" | "PathBuf" | "OsString"
        | "HashMap" | "BTreeMap" | "HashSet" | "BTreeSet"
        | "VecDeque" | "LinkedList" | "Cow" => Some(OwnershipClass::Owns),

        // Shared ownership
        "Arc" | "Rc" => Some(OwnershipClass::Shared),

        // Recurse into generic arguments
        _ => {
            if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                for arg in &args.args {
                    if let syn::GenericArgument::Type(inner) = arg {
                        if let Some(class) = from_type(inner) {
                            return Some(class);
                        }
                    }
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn check(item: syn::ItemStruct, derives: &[&str]) -> OwnershipClass {
        let derives: Vec<String> = derives.iter().map(|s| s.to_string()).collect();
        infer(&item.fields, &item.generics, &derives)
    }

    #[test]
    fn vec_field_infers_owns() {
        let item: syn::ItemStruct = parse_quote! { struct S { data: Vec<u8> } };
        assert_eq!(check(item, &[]), OwnershipClass::Owns);
    }

    #[test]
    fn string_field_infers_owns() {
        let item: syn::ItemStruct = parse_quote! { struct S { name: String } };
        assert_eq!(check(item, &[]), OwnershipClass::Owns);
    }

    #[test]
    fn hashmap_field_infers_owns() {
        let item: syn::ItemStruct = parse_quote! { struct S { map: std::collections::HashMap<String, u32> } };
        assert_eq!(check(item, &[]), OwnershipClass::Owns);
    }

    #[test]
    fn arc_field_infers_shared() {
        let item: syn::ItemStruct = parse_quote! { struct S { inner: std::sync::Arc<Vec<u8>> } };
        assert_eq!(check(item, &[]), OwnershipClass::Shared);
    }

    #[test]
    fn copy_derive_infers_handle() {
        let item: syn::ItemStruct = parse_quote! { struct S { index: u32 } };
        assert_eq!(check(item, &["Copy"]), OwnershipClass::Handle);
    }

    #[test]
    fn lifetime_param_infers_borrows() {
        let item: syn::ItemStruct = parse_quote! { struct S<'a> { data: &'a str } };
        // Lifetime check takes priority over the reference field.
        assert_eq!(check(item, &[]), OwnershipClass::Borrows);
    }

    #[test]
    fn reference_field_without_lifetime_infers_borrows() {
        // Syntactically unusual but valid in some contexts.
        let item: syn::ItemStruct = parse_quote! { struct S { data: &'static str } };
        assert_eq!(check(item, &[]), OwnershipClass::Borrows);
    }

    #[test]
    fn no_indicators_infers_opaque() {
        let item: syn::ItemStruct = parse_quote! { struct S { x: u32, y: f32 } };
        assert_eq!(check(item, &[]), OwnershipClass::Opaque);
    }

    #[test]
    fn copy_takes_priority_over_vec() {
        // Pathological case: Copy + Vec field. Copy wins since Vec can't be Copy,
        // but if someone wrote this, we trust the derive.
        let item: syn::ItemStruct = parse_quote! { struct S { x: u32 } };
        assert_eq!(check(item, &["Copy", "Clone"]), OwnershipClass::Handle);
    }
}

//! `#[derive(Inspectable)]` — generates a real `impl Inspectable` for a
//! concrete struct or enum. See
//! `.agent_handoffs/debug-inspector/b1-t2/research.md` for the full
//! blueprint (attribute grammar, path-hygiene requirements, enum
//! unit-vs-data-carrying branch).

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, LitStr};

#[proc_macro_derive(Inspectable, attributes(inspect))]
pub fn derive_inspectable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident.clone();

    let expanded = match input.data {
        Data::Struct(data_struct) => derive_struct(&name, data_struct.fields),
        Data::Enum(data_enum) => derive_enum(&name, data_enum),
        Data::Union(_) => {
            syn::Error::new_spanned(name, "#[derive(Inspectable)] does not support unions")
                .to_compile_error()
        }
    };

    TokenStream::from(expanded)
}

/// Parsed `#[inspect(...)]` attributes for a single field.
#[derive(Default)]
struct FieldAttrs {
    label: Option<LitStr>,
    range: Option<(syn::Expr, syn::Expr)>,
    readonly: bool,
    hidden: bool,
}

fn parse_field_attrs(field: &syn::Field) -> syn::Result<FieldAttrs> {
    let mut attrs = FieldAttrs::default();
    for attr in &field.attrs {
        if !attr.path().is_ident("inspect") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("label") {
                let value = meta.value()?;
                attrs.label = Some(value.parse::<LitStr>()?);
                Ok(())
            } else if meta.path.is_ident("range") {
                let value = meta.value()?;
                let range = value.parse::<syn::ExprRange>()?;
                let start = *range
                    .start
                    .ok_or_else(|| meta.error("range requires a start bound"))?;
                let end = *range
                    .end
                    .ok_or_else(|| meta.error("range requires an end bound"))?;
                attrs.range = Some((start, end));
                Ok(())
            } else if meta.path.is_ident("readonly") {
                attrs.readonly = true;
                Ok(())
            } else if meta.path.is_ident("hidden") {
                attrs.hidden = true;
                Ok(())
            } else {
                Err(meta.error("unknown #[inspect(..)] key"))
            }
        })?;
    }
    Ok(attrs)
}

fn derive_struct(name: &syn::Ident, fields: Fields) -> proc_macro2::TokenStream {
    let named = match fields {
        Fields::Named(named) => named.named,
        Fields::Unit => Default::default(),
        Fields::Unnamed(_) => {
            return syn::Error::new_spanned(
                name,
                "#[derive(Inspectable)] does not support tuple structs",
            )
            .to_compile_error();
        }
    };

    let mut schema_pushes = Vec::new();
    let mut snapshot_inserts = Vec::new();
    let mut patch_arms = Vec::new();

    for field in named.iter() {
        let attrs = match parse_field_attrs(field) {
            Ok(attrs) => attrs,
            Err(err) => return err.to_compile_error(),
        };
        let ident = field
            .ident
            .clone()
            .expect("Fields::Named always has an ident");
        let ident_str = ident.to_string();
        let ty = &field.ty;

        if attrs.hidden {
            // Hidden fields are absent from schema, snapshot, and apply_patch
            // entirely (MUST-2).
            continue;
        }

        let label_set = attrs.label.as_ref().map(|label| {
            quote! { f.label = Some(#label.to_string()); }
        });
        let range_set = attrs.range.as_ref().map(|(lo, hi)| {
            quote! {
                f.range = Some(::scene_core::Range { min: (#lo) as f64, max: (#hi) as f64 });
            }
        });
        let readonly_set = if attrs.readonly {
            Some(quote! { f.readonly = true; })
        } else {
            None
        };

        schema_pushes.push(quote! {
            let mut f = <#ty as ::scene_core::Inspectable>::schema();
            f.name = #ident_str.to_string();
            #label_set
            #range_set
            #readonly_set
            ch.push(f);
        });

        snapshot_inserts.push(quote! {
            m.insert(#ident_str.to_string(), ::scene_core::Inspectable::snapshot(&self.#ident));
        });

        if attrs.readonly {
            patch_arms.push(quote! {
                #ident_str => Err(::scene_core::PatchError::Readonly),
            });
        } else {
            patch_arms.push(quote! {
                #ident_str => ::scene_core::Inspectable::apply_patch(
                    &mut self.#ident,
                    tail.unwrap_or(""),
                    value,
                ),
            });
        }
    }

    quote! {
        impl ::scene_core::Inspectable for #name {
            fn schema() -> ::scene_core::FieldSchema
            where
                Self: Sized,
            {
                ::scene_core::FieldSchema {
                    name: String::new(),
                    label: None,
                    tag: ::scene_core::FieldTag::Struct,
                    readonly: false,
                    hidden: false,
                    range: None,
                    variants: Vec::new(),
                    children: {
                        let mut ch = Vec::new();
                        #(#schema_pushes)*
                        ch
                    },
                }
            }

            fn snapshot(&self) -> ::scene_core::__private::serde_json::Value {
                let mut m = ::scene_core::__private::serde_json::Map::new();
                #(#snapshot_inserts)*
                ::scene_core::__private::serde_json::Value::Object(m)
            }

            fn apply_patch(
                &mut self,
                path: &str,
                value: ::scene_core::__private::serde_json::Value,
            ) -> Result<(), ::scene_core::PatchError> {
                let (seg, tail) = ::scene_core::parse_path_segment(path)?;
                let field = match seg {
                    ::scene_core::Segment::Field(f) => f,
                    ::scene_core::Segment::Index(_) => {
                        return Err(::scene_core::PatchError::NotIndexable)
                    }
                };
                match field {
                    #(#patch_arms)*
                    _ => Err(::scene_core::PatchError::UnknownField(field.to_string())),
                }
            }
        }
    }
}

fn derive_enum(name: &syn::Ident, data_enum: syn::DataEnum) -> proc_macro2::TokenStream {
    let all_unit = data_enum
        .variants
        .iter()
        .all(|v| matches!(v.fields, Fields::Unit));

    if !all_unit {
        return quote! {
            impl ::scene_core::Inspectable for #name {
                fn schema() -> ::scene_core::FieldSchema
                where
                    Self: Sized,
                {
                    ::scene_core::FieldSchema {
                        name: String::new(),
                        label: None,
                        tag: ::scene_core::FieldTag::Json,
                        readonly: true,
                        hidden: false,
                        range: None,
                        children: Vec::new(),
                        variants: Vec::new(),
                    }
                }

                fn snapshot(&self) -> ::scene_core::__private::serde_json::Value {
                    ::scene_core::__private::serde_json::to_value(self)
                        .expect("Json-fallback Inspectable enum must be Serialize")
                }

                fn apply_patch(
                    &mut self,
                    _path: &str,
                    _value: ::scene_core::__private::serde_json::Value,
                ) -> Result<(), ::scene_core::PatchError> {
                    Err(::scene_core::PatchError::Readonly)
                }
            }
        };
    }

    let variant_idents: Vec<&syn::Ident> = data_enum.variants.iter().map(|v| &v.ident).collect();
    let variant_names: Vec<String> = variant_idents.iter().map(|v| v.to_string()).collect();

    let snapshot_arms = variant_idents.iter().zip(variant_names.iter()).map(|(id, n)| {
        quote! { #name::#id => #n, }
    });

    let from_name_arms = variant_idents.iter().zip(variant_names.iter()).map(|(id, n)| {
        quote! { #n => #name::#id, }
    });

    quote! {
        impl ::scene_core::Inspectable for #name {
            fn schema() -> ::scene_core::FieldSchema
            where
                Self: Sized,
            {
                ::scene_core::FieldSchema {
                    name: String::new(),
                    label: None,
                    tag: ::scene_core::FieldTag::Enum,
                    readonly: false,
                    hidden: false,
                    range: None,
                    children: Vec::new(),
                    variants: vec![#(#variant_names.to_string()),*],
                }
            }

            fn snapshot(&self) -> ::scene_core::__private::serde_json::Value {
                let name = match self {
                    #(#snapshot_arms)*
                };
                ::scene_core::__private::serde_json::Value::String(name.to_string())
            }

            fn apply_patch(
                &mut self,
                path: &str,
                value: ::scene_core::__private::serde_json::Value,
            ) -> Result<(), ::scene_core::PatchError> {
                if !path.is_empty() {
                    return Err(::scene_core::PatchError::NotIndexable);
                }
                let variant_name = value
                    .as_str()
                    .ok_or(::scene_core::PatchError::TypeMismatch)?;
                *self = match variant_name {
                    #(#from_name_arms)*
                    _ => return Err(::scene_core::PatchError::TypeMismatch),
                };
                Ok(())
            }
        }
    }
}

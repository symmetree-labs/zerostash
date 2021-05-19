use proc_macro2::TokenStream;
use quote::quote;
use syn::{Attribute, Data, DataStruct, DeriveInput, Fields, Ident, Lit, Meta, NestedMeta};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let fields = match input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(fields),
            ..
        }) => fields.named,
        _ => panic!("this derive macro only works on structs with named fields"),
    };

    let getters = fields
        .into_iter()
        .map(|f| {
            let attrs: Vec<_> = f
                .attrs
                .iter()
                .filter(|attr| attr.path.is_ident("getter"))
                .collect();

            let name_from_attr = match attrs.len() {
                0 => None,
                1 => get_name_attr(&attrs[0])?,
                _ => {
                    let mut error =
                        syn::Error::new_spanned(&attrs[1], "redundant `getter(name)` attribute");
                    error.combine(syn::Error::new_spanned(&attrs[0], "note: first one here"));
                    return Err(error);
                }
            };

            // if there is no `getter(name)` attribute use the field name like before
            let method_name =
                name_from_attr.unwrap_or_else(|| f.ident.clone().expect("a named field"));
            let field_name = f.ident;
            let field_ty = f.ty;

            Ok(quote! {
                pub fn #method_name(&self) -> &#field_ty {
                    &self.#field_name
                }
            })
        })
        .collect::<syn::Result<TokenStream>>()?;

    let st_name = input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #st_name #ty_generics #where_clause {
            #getters
        }
    })
}

fn get_name_attr(attr: &Attribute) -> syn::Result<Option<Ident>> {
    let meta = attr.parse_meta()?;
    let meta_list = match meta {
        Meta::List(list) => list,
        _ => {
            return Err(syn::Error::new_spanned(
                meta,
                "expected a list-style attribute",
            ))
        }
    };

    let nested = match meta_list.nested.len() {
        // `#[getter()]` without any arguments is a no-op
        0 => return Ok(None),
        1 => &meta_list.nested[0],
        _ => {
            return Err(syn::Error::new_spanned(
                meta_list.nested,
                "currently only a single getter attribute is supported",
            ));
        }
    };

    let name_value = match nested {
        NestedMeta::Meta(Meta::NameValue(nv)) => nv,
        _ => {
            return Err(syn::Error::new_spanned(
                nested,
                "expected `name = \"<value>\"`",
            ))
        }
    };

    if !name_value.path.is_ident("name") {
        return Err(syn::Error::new_spanned(
            &name_value.path,
            "unsupported getter attribute, expected `name`",
        ));
    }

    match &name_value.lit {
        Lit::Str(s) => syn::parse_str(&s.value()).map_err(|e| syn::Error::new_spanned(s, e)),
        lit => Err(syn::Error::new_spanned(lit, "")),
    }
}

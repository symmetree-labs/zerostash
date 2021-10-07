#[rustfmt::skip]

use proc_macro2::{Span, TokenStream};
use proc_macro_crate::{crate_name, FoundCrate};
use quote::quote;
use syn::{
    Attribute, Data, DataStruct, DeriveInput, Field, Fields, Ident, Lit, LitStr, Meta, NestedMeta,
};

struct StructField {
    field: Field,
    skip: bool,
    rename: String,
    strategy: TokenStream,
}

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let infinitree_crate = match crate_name("infinitree").expect("couldn't find infinitree") {
        FoundCrate::Itself => quote!(crate),
        FoundCrate::Name(name) => {
            let ident = Ident::new(&name, Span::call_site());
            quote!( ::#ident )
        }
    };

    let fields = match input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(fields),
            ..
        }) => fields.named,
        _ => panic!("this derive macro only works on structs with named fields"),
    };

    let fields = fields
        .into_iter()
        .filter_map(|f| {
            let field = f
                .attrs
                .iter()
                .filter(|attr| attr.path.is_ident("infinitree"))
                .fold(
                    StructField {
                        field: f.clone(),
                        skip: false,
                        rename: f.ident.expect("named field expected").to_string(),
                        strategy: quote! ( #infinitree_crate::index::LocalField ),
                    },
                    |mut field, attr| {
                        if let Ok(Some(rename)) = get_name_attr(attr) {
                            field.rename = rename.to_string();
                        }

                        if let Ok(Some(strategy)) = get_strategy_attr(attr) {
                            field.strategy = quote!( #strategy );
                        }

                        if let Ok(true) = should_skip(attr) {
                            field.skip = true;
                        }

                        field
                    },
                );

            match field.skip {
                false => Some(field),
                true => None,
            }
        })
        .collect::<Vec<_>>();

    let getters = fields
        .iter()
        .map(|f| {
            let method_name = Ident::new(&f.rename, Span::mixed_site());
            let field_name_str = Lit::Str(LitStr::new(f.rename.as_str(), Span::mixed_site()));
            let field_name = &f.field.ident;
            let field_ty = &f.field.ty;
            let strategy = &f.strategy;

            Ok(quote! {
		#[inline]
                pub fn #method_name(&'_ self) -> #infinitree_crate::index::Access<Box<#strategy<#field_ty>>> {
		    use #infinitree_crate::index::{Access, Strategy};
		    Access::new(
			#field_name_str,
			Box::new(#strategy::for_field(
			    &self.#field_name,
			)),
		    )
                }
            })
        })
        .collect::<syn::Result<TokenStream>>()?;

    let strategies = fields
        .iter()
        .map(|f| {
            let field_name = Ident::new(&f.rename, Span::mixed_site());
            quote! { self.#field_name().into(), }
        })
        .collect::<TokenStream>();

    let field_name_list = fields
        .iter()
        .map(|f| {
            let field_name_str = Lit::Str(LitStr::new(f.rename.as_str(), Span::mixed_site()));
            quote! { #field_name_str.into(), }
        })
        .collect::<TokenStream>();

    let st_name = input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    {
        Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #st_name #ty_generics #where_clause {
            #getters

            pub fn fields(&self) -> Vec<String> {
                vec![#field_name_list]
            }
        }


        impl #infinitree_crate::Index for #impl_generics #st_name #ty_generics #where_clause {
            fn store_all(&'_ mut self) -> #infinitree_crate::anyhow::Result<Vec<#infinitree_crate::index::Access<Box<dyn #infinitree_crate::index::Store>>>> {
                Ok(vec![#strategies])
            }

            fn load_all(&'_ mut self) -> #infinitree_crate::anyhow::Result<Vec<#infinitree_crate::index::Access<Box<dyn #infinitree_crate::index::Load>>>> {
                Ok(vec![#strategies])
            }
        }
        })
    }
}

fn get_attr(attr: &Attribute) -> syn::Result<Option<NestedMeta>> {
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

    match meta_list.nested.len() {
        // `#[stash()]` without any arguments is a no-op
        0 => Ok(None),
        1 => Ok(Some(meta_list.nested[0].clone())),
        _ => Err(syn::Error::new_spanned(
            meta_list.nested,
            "currently only a single stash attribute is supported",
        )),
    }
}

fn get_strategy_attr(attr: &Attribute) -> syn::Result<Option<syn::Type>> {
    let name_value = match get_attr(attr)? {
        Some(NestedMeta::Meta(Meta::NameValue(nv))) => nv,
        _ => return Ok(None),
    };

    if !name_value.path.is_ident("strategy") {
        return Err(syn::Error::new_spanned(
            &name_value.path,
            "unsupported attribute; expected `strategy`",
        ));
    }

    match &name_value.lit {
        Lit::Str(s) => syn::parse_str(&s.value())
            .map(Some)
            .map_err(|e| syn::Error::new_spanned(s, e)),
        lit => Err(syn::Error::new_spanned(lit, "")),
    }
}

fn get_name_attr(attr: &Attribute) -> syn::Result<Option<Ident>> {
    let name_value = match get_attr(attr)? {
        Some(NestedMeta::Meta(Meta::NameValue(nv))) => nv,
        _ => return Ok(None),
    };

    if !name_value.path.is_ident("name") {
        return Err(syn::Error::new_spanned(
            &name_value.path,
            "unsupported attribute; expected `name`",
        ));
    }

    match &name_value.lit {
        Lit::Str(s) => syn::parse_str(&s.value()).map_err(|e| syn::Error::new_spanned(s, e)),
        lit => Err(syn::Error::new_spanned(lit, "")),
    }
}

fn should_skip(attr: &Attribute) -> syn::Result<bool> {
    let skip_value = match get_attr(attr) {
        Ok(Some(NestedMeta::Meta(Meta::Path(path)))) => path,
        _ => {
            return Err(syn::Error::new_spanned(
                &attr,
                "unexpected attribute; expected `skip`",
            ))
        }
    };

    Ok(skip_value.is_ident("skip"))
}

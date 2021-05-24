#[rustfmt::skip]

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    Attribute, Data, DataStruct, DeriveInput, Field, Fields, Ident, Lit, LitStr, Meta, NestedMeta,
};

struct StructField {
    field: Field,
    skip: bool,
    rename: String,
}

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
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
                .filter(|attr| attr.path.is_ident("stash"))
                .fold(
                    StructField {
                        field: f.clone(),
                        skip: false,
                        rename: f.ident.expect("named field expected").to_string(),
                    },
                    |mut field, attr| {
                        if let Ok(Some(rename)) = get_name_attr(attr) {
                            field.rename = rename.to_string();
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
            let field_name = &f.field.ident;
            let field_ty = &f.field.ty;

            Ok(quote! {
                pub fn #method_name(&self) -> &#field_ty {
                    &self.#field_name
                }
            })
        })
        .collect::<syn::Result<TokenStream>>()?;

    let writers = fields
        .iter()
        .map(|f| {
            let field_name = &f.field.ident;
            let field_name_str = Lit::Str(LitStr::new(f.rename.as_str(), Span::mixed_site()));

            quote! { metawriter.write_field(#field_name_str, &self.#field_name).await; }
        })
        .collect::<TokenStream>();

    let readers = fields
        .iter()
        .map(|f| {
            let field_name = &f.field.ident;
            let field_name_str = Lit::Str(LitStr::new(f.rename.as_str(), Span::mixed_site()));

            quote! {
            match metareader.read_into(#field_name_str, &mut self.#field_name).await {
                Ok(_) | Err(libzerostash::index::ReadError::NoField) => (),
                Err(e) => return Err(e.into()),
            };
            }
        })
        .collect::<TokenStream>();

    let st_name = input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    {
        Ok(quote! {
        use libzerostash::async_trait;
            #[automatically_derived]
            impl #impl_generics #st_name #ty_generics #where_clause {
                #getters
            }

        #[async_trait]
        impl libzerostash::Index for #impl_generics #st_name #ty_generics #where_clause {
                async fn read_fields(
            &mut self,
            mut metareader: libzerostash::index::Reader,
            start_object: libzerostash::ObjectId,
                ) -> Result<()> {
            let mut next_object = Some(start_object);

            while let Some(header) = match next_object {
                Some(ref o) => Some(metareader.open(o).await?),
                None => None,
            } {
                next_object = header.next_object();
                #readers
            }

            Ok(())
                }

                async fn write_fields(&mut self, metawriter: &mut libzerostash::index::Writer) -> Result<()> {
            #writers

            metawriter.seal_and_store().await;
            Ok(())
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

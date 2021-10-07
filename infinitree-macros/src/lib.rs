#![forbid(unsafe_code)]
#![deny(clippy::all)]

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod derive_index;

#[proc_macro_derive(Index, attributes(infinitree))]
pub fn derive_index_macro(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_index::expand(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_macro() {
        use quote::quote;
        use syn::parse_quote;

        let input = parse_quote! {
        #[derive(Default, Index)]
        pub struct TestStruct {
            /// A field with both an accessor method and serialized to storage
            unattributed: ChunkIndex,

            /// Rename the field to `renamed_chunks` both in serialized form
            /// and accessor method
            #[infinitree(name = "renamed_chunks")]
            chunks: ChunkIndex,

            /// Skip generating accessors and exclude from on-disk structure
            #[infinitree(skip)]
            _unreferenced: ChunkIndex,

            /// Skip generating accessors and exclude from on-disk structure
            #[infinitree(strategy = "infinitree::index::SparseField")]
            strategizing: ChunkIndex,
        }
        };

        let result = super::derive_index::expand(input).unwrap();

        #[rustfmt::skip]
        let expected = quote! {
        #[automatically_derived]
        impl TestStruct {
            #[inline]
            pub fn unattributed(&'_ self) -> ::infinitree::index::Access<Box<::infinitree::index::LocalField<ChunkIndex>>> {
                use ::infinitree::index::{Access, Strategy};
                Access::new(
                    "unattributed",
                    Box::new(::infinitree::index::LocalField::for_field(
			&self.unattributed,
		    )),
                )
            }
            #[inline]
            pub fn renamed_chunks(&'_ self) -> ::infinitree::index::Access<Box<::infinitree::index::LocalField<ChunkIndex>>> {
                use ::infinitree::index::{Access, Strategy};
                Access::new(
                    "renamed_chunks",
                    Box::new(::infinitree::index::LocalField::for_field(
			&self.chunks,
		    )),
                )
            }
            #[inline]
            pub fn strategizing(&'_ self) -> ::infinitree::index::Access<Box<infinitree::index::SparseField<ChunkIndex>>> {
                use ::infinitree::index::{Access, Strategy};
                Access::new(
                    "strategizing",
                    Box::new(infinitree::index::SparseField::for_field(
                        &self.strategizing,
                    )),
                )
            }
            pub fn fields(&self) -> Vec<String> {
                vec!["unattributed".into(),
		     "renamed_chunks".into(),
		     "strategizing".into(),
		]
            }
        }
        impl ::infinitree::Index for TestStruct {
            fn store_all(&'_ mut self) -> ::infinitree::anyhow::Result<Vec<::infinitree::index::Access<Box<dyn ::infinitree::index::Store>>>> {
                Ok(vec![
                    self.unattributed().into(),
                    self.renamed_chunks().into(),
                    self.strategizing().into(),
                ])
            }
            fn load_all(&'_ mut self) -> ::infinitree::anyhow::Result<Vec<::infinitree::index::Access<Box<dyn ::infinitree::index::Load>>>> {
                Ok(vec![
                    self.unattributed().into(),
                    self.renamed_chunks().into(),
                    self.strategizing().into(),
                ])
            }
        }
            };

        assert_eq!(result.to_string(), expected.to_string());
    }
}

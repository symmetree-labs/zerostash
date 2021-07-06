use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod derive_index;

#[proc_macro_derive(Index, attributes(skip))]
pub fn stash(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_index::expand(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

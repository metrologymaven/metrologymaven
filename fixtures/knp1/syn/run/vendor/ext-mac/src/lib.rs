use proc_macro::TokenStream;

#[proc_macro]
pub fn fuse(input: TokenStream) -> TokenStream {
    let _ = quote::quote! { a.mul_add(b, c) };
    input
}

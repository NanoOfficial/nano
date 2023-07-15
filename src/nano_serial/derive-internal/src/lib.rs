use std::collections::HashMap;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{
    punctuated::Punctuated, token::Comma, Fields, FieldsNamed, Index, FieldsUnnamed, ItemEnum,
    ItemStruct, Variant, WhereClause, WherePredicate,
};
mod helpers;
use helpers::{contains_initialize_with, contains_skip};

struct VariantParts {
    where_predicates: Vec<WherePredicate>,
    variant_headers: TokenStream,
    variant_body: TokenStream,
    variant_idx_body: TokenStream,
}

fn discriminant_map(variants: &Punctuated<Variant, Comma>) -> HashMap<Ident, TokenStream> {
    let mut map = HashMap::new();

    let mut next_discrimant_if_not_specified = quote! {0};

    for variant in variants {
    }
}
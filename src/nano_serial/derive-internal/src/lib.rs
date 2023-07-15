use std::collections::HashMap;

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{
    punctuated::Punctuated, token::Comma, Fields, FieldsNamed, FieldsUnnamed, Index, ItemEnum,
    ItemStruct, Variant, WhereClause, WherePredicate,
};

mod helpers;
use helpers::{contains_initialize_with, contains_skip};

struct VariantParts {
    where_predicates: Vec<WherePredicate>,
    variant_header: TokenStream,
    variant_body: TokenStream,
    variant_idx_body: TokenStream,
}

fn discriminant_map(variants: &Punctuated<Variant, Comma>) -> HashMap<Ident, TokenStream> {
    let mut map = HashMap::new();

    let mut next_discriminant_if_not_specified = quote! {0};

    for variant in variants {
        let this_discriminant = variant
            .discriminant
            .clone()
            .map_or_else(|| quote! { #next_discriminant_if_not_specified }, |(_, e)| quote! { #e });

        next_discriminant_if_not_specified = quote! { #this_discriminant + 1 };
        map.insert(variant.ident.clone(), this_discriminant);
    }

    map
}

fn named_fields(
    cratename: &Ident,
    enum_ident: &Ident,
    variant_ident: &Ident,
    discriminant_value: &TokenStream,
    fields: &FieldsNamed,
) -> syn::Result<VariantParts> {
    let mut where_predicates: Vec<WherePredicate> = vec![];
    let mut variant_header = TokenStream::new();
    let mut variant_body = TokenStream::new();

    for field in &fields.named {
        if !contains_skip(&field.attrs) {
            let field_ident = field.ident.clone().unwrap();

            variant_header.extend(quote! { #field_ident, });

            let field_type = &field.ty;
            where_predicates.push(
                syn::parse2(quote! {
                    #field_type: #cratename::Encodable
                })
                .unwrap(),
            );

            variant_body.extend(quote! {
                len += #field_ident.encode(&mut s)?;
            })
        }
    }

    variant_header = quote! { { #variant_header .. }};
    let variant_idx_body = quote!(
        #enum_ident::#variant_ident { .. } => #discriminant_value,
    );

    Ok(VariantParts { where_predicates, variant_header, variant_body, variant_idx_body })
}

fn unnamed_fields(
    cratename: &Ident,
    enum_ident: &Ident,
    variant_ident: &Ident,
    discriminant_value: &TokenStream,
    fields: &FieldsUnnamed,
) -> syn::Result<VariantParts> {
    let mut where_predicates: Vec<WherePredicate> = vec![];
    let mut variant_header = TokenStream::new();
    let mut variant_body = TokenStream::new();

    for (field_idx, field) in fields.unnamed.iter().enumerate() {
        let field_idx = u32::try_from(field_idx).expect("up to 2^32 fields are supported");
        if contains_skip(&field.attrs) {
            let field_ident = Ident::new(format!("_id{}", field_idx).as_str(), Span::mixed_site());
            variant_header.extend(quote! { #field_ident, });
        } else {
            let field_ident = Ident::new(format!("id{}", field_idx).as_str(), Span::mixed_site());
            variant_header.extend(quote! { #field_ident, });

            let field_type = &field.ty;
            where_predicates.push(
                syn::parse2(quote! {
                    #field_type: #cratename::Encodable
                })
                .unwrap(),
            );

            variant_body.extend(quote! {
                len += #field_ident.encode(&mut s)?;
            })
        }
    }

    variant_header = quote! { ( #variant_header )};
    let variant_idx_body = quote!(
        #enum_ident::#variant_ident(..) => #discriminant_value,
    );

    Ok(VariantParts { where_predicates, variant_header, variant_body, variant_idx_body })
}

pub fn enum_ser(input: &ItemEnum, cratename: Ident) -> syn::Result<TokenStream> {
    let enum_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let mut where_clause = where_clause.map_or_else(
        || WhereClause { where_token: Default::default(), predicates: Default::default() },
        Clone::clone,
    );
    let mut all_variants_idx_body = TokenStream::new();
    let mut fields_body = TokenStream::new();
    let discriminants = discriminant_map(&input.variants);

    for variant in input.variants.iter() {
        let variant_ident = &variant.ident;
        let discriminant_value = discriminants.get(variant_ident).unwrap();
        let VariantParts { where_predicates, variant_header, variant_body, variant_idx_body } =
            match &variant.fields {
                Fields::Named(fields) => {
                    named_fields(&cratename, enum_ident, variant_ident, discriminant_value, fields)?
                }
                Fields::Unnamed(fields) => unnamed_fields(
                    &cratename,
                    enum_ident,
                    variant_ident,
                    discriminant_value,
                    fields,
                )?,
                Fields::Unit => {
                    let variant_idx_body = quote!(
                        #enum_ident::#variant_ident => #discriminant_value,
                    );
                    VariantParts {
                        where_predicates: vec![],
                        variant_header: TokenStream::new(),
                        variant_body: TokenStream::new(),
                        variant_idx_body,
                    }
                }
            };
        where_predicates.into_iter().for_each(|predicate| where_clause.predicates.push(predicate));
        all_variants_idx_body.extend(variant_idx_body);
        fields_body.extend(quote!(
            #enum_ident::#variant_ident #variant_header => {
                #variant_body
            }
        ))
    }

    Ok(quote! {
        impl #impl_generics #cratename::Encodable for #enum_ident #ty_generics #where_clause {
            fn encode<S: std::io::Write>(&self, mut s: S) -> ::core::result::Result<usize, std::io::Error> {
                let variant_idx: u8 = match self {
                    #all_variants_idx_body
                };

                let mut len = 0;
                let bytes = variant_idx.to_le_bytes();
                s.write_all(&bytes)?;
                len += bytes.len();

                match self {
                    #fields_body
                }

                Ok(len)
            }
        }
    })
}

pub fn enum_de(input: &ItemEnum, cratename: Ident) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let mut where_clause = where_clause.map_or_else(
        || WhereClause { where_token: Default::default(), predicates: Default::default() },
        Clone::clone,
    );

    let init_method = contains_initialize_with(&input.attrs);
    let mut variant_arms = TokenStream::new();
    let discriminants = discriminant_map(&input.variants);

    for variant in input.variants.iter() {
        let variant_ident = &variant.ident;
        let discriminant = discriminants.get(variant_ident).unwrap();
        let mut variant_header = TokenStream::new();
        match &variant.fields {
            Fields::Named(fields) => {
                for field in &fields.named {
                    let field_name = field.ident.as_ref().unwrap();
                    if contains_skip(&field.attrs) {
                        variant_header.extend(quote! {
                            #field_name: Default::default(),
                        });
                    } else {
                        let field_type = &field.ty;
                        where_clause.predicates.push(
                            syn::parse2(quote! {
                                #field_type: #cratename::Decodable
                            })
                            .unwrap(),
                        );

                        variant_header.extend(quote! {
                            #field_name: #cratename::Decodable::decode(&mut d)?,
                        });
                    }
                }
                variant_header = quote! { { #variant_header }};
            }
            Fields::Unnamed(fields) => {
                for field in fields.unnamed.iter() {
                    if contains_skip(&field.attrs) {
                        variant_header.extend(quote! { Default::default(), });
                    } else {
                        let field_type = &field.ty;
                        where_clause.predicates.push(
                            syn::parse2(quote! {
                                #field_type: #cratename::Decodable
                            })
                            .unwrap(),
                        );

                        variant_header.extend(quote! {
                            #cratename::Decodable::decode(&mut d)?,
                        });
                    }
                }
                variant_header = quote! { ( #variant_header )};
            }
            Fields::Unit => {}
        }
        variant_arms.extend(quote! {
            if variant_tag == #discriminant { #name::#variant_ident #variant_header } else
        });
    }

    let init = if let Some(method_ident) = init_method {
        quote! {
            return_value.#method_ident();
        }
    } else {
        quote! {}
    };

    Ok(quote! {
    impl #impl_generics #cratename::Decodable for #name #ty_generics #where_clause {
        fn decode<D: std::io::Read>(mut d: D) -> ::core::result::Result<Self, std::io::Error> {
            let variant_tag: u8 = #cratename::Decodable::decode(&mut d)?;

            let mut return_value =
                #variant_arms {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Unexpected variant tag: {:?}", variant_tag),
                    ))
                };
                #init
                Ok(return_value)
            }
        }
    })
}

pub fn struct_ser(input: &ItemStruct, cratename: Ident) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let mut where_clause = where_clause.map_or_else(
        || WhereClause { where_token: Default::default(), predicates: Default::default() },
        Clone::clone,
    );

    let mut body = TokenStream::new();

    match &input.fields {
        Fields::Named(fields) => {
            for field in &fields.named {
                if contains_skip(&field.attrs) {
                    continue
                }

                let field_name = field.ident.as_ref().unwrap();
                let delta = quote! {
                    len += self.#field_name.encode(&mut s)?;
                };
                body.extend(delta);

                let field_type = &field.ty;
                where_clause.predicates.push(
                    syn::parse2(quote! {
                        #field_type: #cratename::Encodable
                    })
                    .unwrap(),
                );
            }
        }
        Fields::Unnamed(fields) => {
            for field_idx in 0..fields.unnamed.len() {
                let field_idx = Index {
                    index: u32::try_from(field_idx).expect("up to 2^32 fields are supported"),
                    span: Span::call_site(),
                };
                let delta = quote! {
                    len += self.#field_idx.encode(&mut s)?;
                };
                body.extend(delta);
            }
        }
        Fields::Unit => {}
    }

    Ok(quote! {
        impl #impl_generics #cratename::Encodable for #name #ty_generics #where_clause {
            fn encode<S: std::io::Write>(&self, mut s: S) -> ::core::result::Result<usize, std::io::Error> {
                let mut len = 0;
                #body
                Ok(len)
            }
        }
    })
}

pub fn struct_de(input: &ItemStruct, cratename: Ident) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let mut where_clause = where_clause.map_or_else(
        || WhereClause { where_token: Default::default(), predicates: Default::default() },
        Clone::clone,
    );

    let init_method = contains_initialize_with(&input.attrs);
    let return_value = match &input.fields {
        Fields::Named(fields) => {
            let mut body = TokenStream::new();
            for field in &fields.named {
                let field_name = field.ident.as_ref().unwrap();

                let delta = if contains_skip(&field.attrs) {
                    quote! {
                        #field_name: Default::default(),
                    }
                } else {
                    let field_type = &field.ty;
                    where_clause.predicates.push(
                        syn::parse2(quote! {
                            #field_type: #cratename::Decodable
                        })
                        .unwrap(),
                    );

                    quote! {
                        #field_name: #cratename::Decodable::decode(&mut d)?,
                    }
                };
                body.extend(delta);
            }
            quote! {
                Self { #body }
            }
        }
        Fields::Unnamed(fields) => {
            let mut body = TokenStream::new();
            for _ in 0..fields.unnamed.len() {
                let delta = quote! {
                    #cratename::Decodable::decode(&mut d)?,
                };
                body.extend(delta);
            }
            quote! {
                Self( #body )
            }
        }
        Fields::Unit => {
            quote! {
                Self {}
            }
        }
    };

    if let Some(method_ident) = init_method {
        Ok(quote! {
        impl #impl_generics #cratename::Decodable for #name #ty_generics #where_clause {
            fn decode<D: std::io::Read>(mut d: D) -> ::core::result::Result<Self, std::io::Error> {
                let mut return_value = #return_value;
                return_value.#method_ident();
                Ok(return_value)
            }
        }
        })
    } else {
        Ok(quote! {
            impl #impl_generics #cratename::Decodable for #name #ty_generics #where_clause {
                fn decode<D: std::io::Read>(mut d: D) -> ::core::result::Result<Self, std::io::Error> {
                    Ok(#return_value)
                }
            }
        })
    }
}
#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use quote::quote;
use syn::{Expr, ItemFn, parse_macro_input};

/// Defines a performance tuning parameter that can be automatically optimized by CovOpt.
///
/// This macro extracts hardcoded magic numbers into dynamically tunable parameters.
/// During normal execution, it evaluates to the `$default` value.
/// During `covopt optimize`, the CLI tool injects environment variables to tune it.
///
/// # Example
/// ```rust
/// use covopt_macro::covopt_param;
///
/// let cache_size = covopt_param!("cache_size", 1024);
/// ```
#[proc_macro]
pub fn covopt_param(input: TokenStream) -> TokenStream {
    let args_str = input.to_string();
    let parts: Vec<&str> = args_str.split(',').collect();
    if parts.len() != 2 {
        panic!("covopt_param! requires exactly 2 arguments: name and default value");
    }

    let name = parts[0].trim().trim_matches('"');
    let env_name = format!("COVOPT_PARAM_{}", name);
    let default_val_str = parts[1].trim();

    // Parse the default value as an expression
    let default_expr: Expr =
        syn::parse_str(default_val_str).expect("Failed to parse default value");

    let expanded = quote! {
        std::env::var(#env_name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(#default_expr)
    };

    TokenStream::from(expanded)
}

/// Marks a function as a CovOpt complexity test.
///
/// This macro wraps the function in a standard `#[test]` and automatically injects
/// the boilerplate code to read the `COVOPT_N` environment variable.
/// The `expected` and `n_values` metadata provided in the attribute are statically
/// parsed by the `covopt` CLI engine during analysis.
///
/// # Example
/// ```rust
/// #[covopt_macro::test(expected = "O(N)")]
/// fn test_my_algorithm(n: usize) {
///     // algorithm body...
/// }
/// ```
#[proc_macro_attribute]
pub fn test(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);

    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;

    // Check if the function has exactly one parameter (e.g. `n: usize`)
    if input_fn.sig.inputs.len() != 1 {
        panic!("#[covopt::test] requires a function with exactly 1 parameter (e.g. `n: usize`)");
    }

    // Wrap the original body in a closure
    let orig_body = &input_fn.block;
    let sig_inputs = &input_fn.sig.inputs;

    let expanded = quote! {
        #[test]
        #fn_vis fn #fn_name() {
            let n: usize = std::env::var("COVOPT_N")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10); // Default to 10 if not set

            let mut __covopt_inner = |#sig_inputs| {
                #orig_body
            };

            __covopt_inner(n);
        }
    };

    TokenStream::from(expanded)
}

/// Explicitly anchor the `CovOpt-Analyzer` static analyzer to a specific line.
///
/// This macro compiles to `std::hint::black_box(())` to prevent dead code elimination
/// of the anchoring mechanism, but its primary purpose is for the `covopt` CLI tool
/// to parse the AST and find exactly which source line it should track hit counts for.
#[proc_macro]
pub fn track(_input: TokenStream) -> TokenStream {
    TokenStream::from(quote! {
        std::hint::black_box(());
    })
}

/// Automatically injects `std::hint::black_box` to prevent Dead Code Elimination (DCE).
///
/// Wraps all input parameters and the final return value in `std::hint::black_box`.
#[proc_macro_attribute]
pub fn no_dce(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input_fn = parse_macro_input!(item as ItemFn);
    let orig_body = input_fn.block;

    let mut param_shadows = proc_macro2::TokenStream::new();
    for input in &input_fn.sig.inputs {
        if let syn::FnArg::Typed(pat_type) = input {
            if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                let ident = &pat_ident.ident;
                param_shadows.extend(quote! {
                    #[allow(unused_mut)]
                    let mut #ident = std::hint::black_box(#ident);
                });
            }
        }
    }

    let expanded_body = quote! {
        {
            #param_shadows
            let __covopt_res = #orig_body;
            std::hint::black_box(__covopt_res)
        }
    };
    input_fn.block = syn::parse2(expanded_body).unwrap();
    TokenStream::from(quote! { #input_fn })
}

/// Generates a Structure of Arrays (SoA) variant for a struct.
#[proc_macro_derive(SoA)]
pub fn derive_soa(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    let name = &input.ident;
    let soa_name = syn::Ident::new(&format!("{}Soa", name), name.span());

    let fields = if let syn::Data::Struct(syn::DataStruct {
        fields: syn::Fields::Named(fields),
        ..
    }) = &input.data
    {
        fields.named.iter().map(|f| {
            let fname = &f.ident;
            let ftype = &f.ty;
            quote! { pub #fname: Vec<#ftype> }
        })
    } else {
        panic!("SoA derive only supports structs with named fields");
    };

    let expanded = quote! {
        pub struct #soa_name {
            #(#fields,)*
        }
    };

    TokenStream::from(expanded)
}

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
/// use covopt_macro::covopt_test;
///
/// #[covopt_test(target_fn = "test_my_algorithm", expected = "ON")]
/// fn test_my_algorithm(n: usize) {
///     // algorithm body...
/// }
/// ```
#[proc_macro_attribute]
pub fn covopt_test(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);

    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;

    // Check if the function has exactly one parameter (e.g. `n: usize`)
    if input_fn.sig.inputs.len() != 1 {
        panic!("#[covopt_test] requires a function with exactly 1 parameter (e.g. `n: usize`)");
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



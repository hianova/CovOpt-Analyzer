#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote};
use std::collections::BTreeSet;
use syn::parse::{Parse, ParseStream, Parser};
use syn::{Expr, ExprRange, ItemFn, LitStr, Token, parse_macro_input};

/// Declares a parameter default and optional search metadata for CovOpt.
///
/// During normal compilation, it evaluates to the `$default` value. Runtime
/// search/robustness modes may inject a trial value; compile-time confirmation
/// additionally requires a candidate hash. Ordinary builds never opt into a
/// candidate merely because an environment variable happens to be present.
///
/// # Example
/// ```rust
/// use covopt_macro::covopt_param;
///
/// let cache_size = covopt_param!("cache_size", 1024);
/// ```
struct ParamInput {
    id: LitStr,
    default: Expr,
    range: Option<ExprRange>,
    class: Option<String>,
    scale: Option<String>,
    unit: Option<String>,
    risk: Vec<String>,
    evaluation: Option<String>,
}

impl Parse for ParamInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let id: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let default: Expr = input.parse()?;
        let mut range = None;
        let mut class = None;
        let mut scale = None;
        let mut unit = None;
        let mut risk = Vec::new();
        let mut evaluation = None;
        let mut keys = BTreeSet::new();

        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.peek(syn::Ident) && input.peek2(Token![=]) {
                let key: syn::Ident = input.parse()?;
                input.parse::<Token![=]>()?;
                if !keys.insert(key.to_string()) {
                    return Err(syn::Error::new(key.span(), "duplicate covopt_param key"));
                }
                match key.to_string().as_str() {
                    "class" | "scale" | "unit" | "evaluation" => {
                        let value: LitStr = input.parse()?;
                        match key.to_string().as_str() {
                            "class" => class = Some(value.value()),
                            "scale" => scale = Some(value.value()),
                            "unit" => unit = Some(value.value()),
                            _ => evaluation = Some(value.value()),
                        }
                    }
                    "range" => {
                        let value: Expr = input.parse()?;
                        let Expr::Range(value) = value else {
                            return Err(syn::Error::new_spanned(
                                value,
                                "range must be a Rust range expression",
                            ));
                        };
                        range = Some(value);
                    }
                    "risk" => {
                        let values: syn::ExprArray = input.parse()?;
                        for value in values.elems {
                            let Expr::Lit(syn::ExprLit {
                                lit: syn::Lit::Str(value),
                                ..
                            }) = value
                            else {
                                return Err(syn::Error::new_spanned(
                                    value,
                                    "risk entries must be string literals",
                                ));
                            };
                            if value.value().trim().is_empty() {
                                return Err(syn::Error::new_spanned(
                                    value,
                                    "risk/tag entries must not be empty",
                                ));
                            }
                            risk.push(value.value());
                        }
                    }
                    other => {
                        return Err(syn::Error::new(
                            key.span(),
                            format!("unknown covopt_param key `{other}`"),
                        ));
                    }
                }
            } else if range.is_none() {
                let value: Expr = input.parse()?;
                let Expr::Range(value) = value else {
                    return Err(syn::Error::new_spanned(
                        value,
                        "expected a range or named covopt_param option",
                    ));
                };
                range = Some(value);
            } else {
                return Err(input.error("unexpected covopt_param argument"));
            }
        }
        Ok(Self {
            id,
            default,
            range,
            class,
            scale,
            unit,
            risk,
            evaluation,
        })
    }
}

fn valid_parameter_id(id: &str) -> bool {
    !id.is_empty()
        && !id.chars().any(char::is_whitespace)
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '.'))
}

fn validate_range(range: &ExprRange) -> syn::Result<()> {
    let Some(start) = range.start.as_deref() else {
        return Err(syn::Error::new_spanned(
            range,
            "parameter range needs a lower bound",
        ));
    };
    let Some(end) = range.end.as_deref() else {
        return Err(syn::Error::new_spanned(
            range,
            "parameter range needs an upper bound",
        ));
    };
    fn numeric_value(expr: &Expr) -> Option<f64> {
        match expr {
            Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(value),
                ..
            }) => value.base10_parse::<f64>().ok(),
            Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Float(value),
                ..
            }) => value.base10_parse::<f64>().ok(),
            Expr::Unary(syn::ExprUnary {
                op: syn::UnOp::Neg(_),
                expr,
                ..
            }) => numeric_value(expr).map(|value| -value),
            _ => None,
        }
    }
    let (Some(start_value), Some(end_value)) = (numeric_value(start), numeric_value(end)) else {
        return Err(syn::Error::new_spanned(
            range,
            "parameter range bounds must be numeric literals",
        ));
    };
    if !start_value.is_finite() || !end_value.is_finite() || start_value > end_value {
        return Err(syn::Error::new_spanned(
            range,
            "parameter range must have finite bounds with min <= max",
        ));
    }
    Ok(())
}

fn parameter_mode() -> String {
    std::env::var("COVOPT_PARAM_MODE")
        .unwrap_or_else(|_| "default".to_string())
        .to_ascii_lowercase()
}

fn confirm_default(input: &ParamInput) -> syn::Result<TokenStream2> {
    let default = &input.default;
    if !matches!(parameter_mode().as_str(), "confirm" | "robustness") {
        return Ok(quote!(#default));
    }
    if std::env::var_os("COVOPT_CONFIRM_CANDIDATE_HASH").is_none() {
        return Ok(quote!(#default));
    }
    let env_key = format!(
        "COVOPT_CONFIRM_{}",
        input.id.value().replace([':', '-', '.'], "_")
    );
    let Some(value) = std::env::var_os(&env_key) else {
        return Ok(quote!(#default));
    };
    syn::parse_str::<Expr>(&value.to_string_lossy())
        .map(|expr| quote!(#expr))
        .map_err(|error| {
            syn::Error::new(
                input.id.span(),
                format!("invalid confirmed value in {env_key}: {error}"),
            )
        })
}

fn parameter_metadata(input: &ParamInput) -> String {
    let range = input
        .range
        .as_ref()
        .map(ToTokens::to_token_stream)
        .map(|value| value.to_string())
        .unwrap_or_default();
    let class = input.class.as_deref().unwrap_or("unknown");
    let evaluation = input.evaluation.as_deref().unwrap_or("runtime");
    let scale = input.scale.as_deref().unwrap_or("linear");
    let unit = input.unit.as_deref().unwrap_or("");
    format!(
        "covopt.schema={};kind=parameter;id={};class={class};evaluation={evaluation};scale={scale};unit={unit};range={range};risk={}",
        covopt_schema::SCHEMA_VERSION,
        input.id.value(),
        input.risk.join(",")
    )
}

#[proc_macro]
pub fn covopt_param(input: TokenStream) -> TokenStream {
    let parsed = match syn::parse::<ParamInput>(input) {
        Ok(parsed) => parsed,
        Err(error) => return error.to_compile_error().into(),
    };
    if !valid_parameter_id(&parsed.id.value()) {
        return syn::Error::new(
            parsed.id.span(),
            "parameter ID must be non-empty and contain only [A-Za-z0-9_.:-]",
        )
        .to_compile_error()
        .into();
    }
    if let Some(range) = &parsed.range
        && let Err(error) = validate_range(range)
    {
        return error.to_compile_error().into();
    }
    if let Some(evaluation) = &parsed.evaluation
        && !matches!(evaluation.as_str(), "runtime" | "compile_time")
    {
        return syn::Error::new(
            parsed.id.span(),
            "evaluation must be `runtime` or `compile_time`",
        )
        .to_compile_error()
        .into();
    }
    if let Some(class) = &parsed.class
        && !matches!(
            class.as_str(),
            "threshold"
                | "capacity"
                | "budget"
                | "timeout"
                | "retry"
                | "tolerance"
                | "coefficient"
                | "seed"
                | "layout"
                | "ordering"
                | "unknown"
        )
    {
        return syn::Error::new(parsed.id.span(), "unknown parameter class")
            .to_compile_error()
            .into();
    }
    if let Some(scale) = &parsed.scale
        && !matches!(scale.as_str(), "linear" | "log" | "pow2")
    {
        return syn::Error::new(parsed.id.span(), "scale must be `linear`, `log`, or `pow2`")
            .to_compile_error()
            .into();
    }
    if let Some(unit) = &parsed.unit
        && unit.trim().is_empty()
    {
        return syn::Error::new(parsed.id.span(), "unit must not be empty")
            .to_compile_error()
            .into();
    }
    // These fields are intentionally consumed here even though the expansion
    // remains a plain expression.  The analyzer extracts the same structured
    // metadata from the source AST and stores it in the shared schema.
    let _metadata = (&parsed.scale, &parsed.unit, &parsed.risk, &parsed.class);
    let default_expr = match confirm_default(&parsed) {
        Ok(expr) => expr,
        Err(error) => return error.to_compile_error().into(),
    };
    let metadata = parameter_metadata(&parsed);
    if matches!(parameter_mode().as_str(), "search" | "robustness")
        && parsed.evaluation.as_deref() != Some("compile_time")
    {
        let env_name = format!("COVOPT_PARAM_{}", parsed.id.value());
        return quote! {
            {
                const _: &str = #metadata;
                ::std::env::var(#env_name)
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(#default_expr)
            }
        }
        .into();
    }
    quote!({ const _: &str = #metadata; #default_expr }).into()
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
pub fn covopt_test(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = match syn::parse::<ItemFn>(item) {
        Ok(input_fn) => input_fn,
        Err(error) => return error.to_compile_error().into(),
    };
    if input_fn.sig.asyncness.is_some() {
        return syn::Error::new_spanned(
            &input_fn.sig,
            "#[covopt_test] does not support async functions",
        )
        .to_compile_error()
        .into();
    }
    if !input_fn.sig.generics.params.is_empty() || input_fn.sig.generics.where_clause.is_some() {
        return syn::Error::new_spanned(
            &input_fn.sig.generics,
            "#[covopt_test] does not support generic functions",
        )
        .to_compile_error()
        .into();
    }
    if !(1..=3).contains(&input_fn.sig.inputs.len())
        || input_fn
            .sig
            .inputs
            .iter()
            .any(|argument| !matches!(argument, syn::FnArg::Typed(_)))
    {
        return syn::Error::new_spanned(
            &input_fn.sig,
            "#[covopt_test] requires one to three typed input parameters (n, optional seed, optional threads)",
        )
        .to_compile_error()
        .into();
    }
    if let Err(error) = parse_test_metadata(attr) {
        return error.to_compile_error().into();
    }
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;
    let attrs = &input_fn.attrs;
    let orig_body = &input_fn.block;
    let sig_inputs = &input_fn.sig.inputs;
    let input_idents = input_fn
        .sig
        .inputs
        .iter()
        .map(|argument| {
            let syn::FnArg::Typed(argument) = argument else {
                unreachable!("typed argument count was validated above")
            };
            let syn::Pat::Ident(pattern) = &*argument.pat else {
                return Err(syn::Error::new_spanned(
                    &argument.pat,
                    "#[covopt_test] requires identifier patterns for injected n/seed/threads arguments",
                ));
            };
            Ok(pattern.ident.clone())
        })
        .collect::<syn::Result<Vec<_>>>();
    let input_idents = match input_idents {
        Ok(value) => value,
        Err(error) => return error.to_compile_error().into(),
    };
    let call_args = quote!(#(#input_idents),*);
    let trial_bindings = match input_idents.as_slice() {
        [n] => quote! {
            let #n: usize = ::std::env::var("COVOPT_N")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| covopt_macro::covopt_param!("COVOPT_TEST_DEFAULT_N", 10));
        },
        [n, seed] => quote! {
            let #n: usize = ::std::env::var("COVOPT_N")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| covopt_macro::covopt_param!("COVOPT_TEST_DEFAULT_N", 10));
            let #seed: u64 = ::std::env::var("COVOPT_SEED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| covopt_macro::covopt_param!("COVOPT_TEST_DEFAULT_SEED", 0u64));
        },
        [n, seed, threads] => quote! {
            let #n: usize = ::std::env::var("COVOPT_N")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| covopt_macro::covopt_param!("COVOPT_TEST_DEFAULT_N", 10));
            let #seed: u64 = ::std::env::var("COVOPT_SEED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| covopt_macro::covopt_param!("COVOPT_TEST_DEFAULT_SEED", 0u64));
            let #threads: usize = ::std::env::var("COVOPT_THREADS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| covopt_macro::covopt_param!("COVOPT_TEST_DEFAULT_THREADS", 1usize));
        },
        _ => unreachable!(),
    };
    let output = &input_fn.sig.output;
    let expanded = quote! {
        #(#attrs)*
        #[test]
        #fn_vis fn #fn_name() #output {
            #trial_bindings
            let __covopt_inner = |#sig_inputs| { #orig_body };
            __covopt_inner(#call_args)
        }
    };
    TokenStream::from(expanded)
}

#[derive(Default)]
struct TestMetadata;

fn parse_test_metadata(attr: TokenStream) -> syn::Result<TestMetadata> {
    let mut seen = BTreeSet::new();
    let parser = syn::meta::parser(|meta| {
        let key = meta
            .path
            .get_ident()
            .map(ToString::to_string)
            .ok_or_else(|| meta.error("metadata keys must be identifiers"))?;
        if !seen.insert(key.clone()) {
            return Err(meta.error("duplicate covopt_test metadata key"));
        }
        let _: Expr = meta.value()?.parse()?;
        if !matches!(
            key.as_str(),
            "target_fn"
                | "expected"
                | "n_values"
                | "seeds"
                | "seed"
                | "threads"
                | "environment"
                | "axes"
        ) {
            return Err(meta.error(format!("unknown covopt_test field `{key}`")));
        }
        Ok(())
    });
    parser.parse(attr).map(|_| TestMetadata)
}

fn parse_string_metadata(
    attr: TokenStream,
    kind: &str,
    allowed: &[&str],
) -> syn::Result<std::collections::BTreeMap<String, String>> {
    let mut values = std::collections::BTreeMap::new();
    let parser = syn::meta::parser(|meta| {
        let key = meta
            .path
            .get_ident()
            .map(ToString::to_string)
            .ok_or_else(|| meta.error("metadata keys must be identifiers"))?;
        if !allowed.contains(&key.as_str()) {
            return Err(meta.error(format!("unknown {kind} field `{key}`")));
        }
        if values.contains_key(&key) {
            return Err(meta.error("duplicate metadata key"));
        }
        let value: Expr = meta.value()?.parse()?;
        values.insert(key, quote::ToTokens::to_token_stream(&value).to_string());
        Ok(())
    });
    parser.parse(attr).map(|_| values)
}

fn metadata_const(
    item_name: &syn::Ident,
    kind: &str,
    values: &std::collections::BTreeMap<String, String>,
) -> (syn::Ident, String) {
    let const_name = syn::Ident::new(
        &format!(
            "__COVOPT_{}_{}_METADATA",
            kind.to_ascii_uppercase(),
            item_name
        ),
        item_name.span(),
    );
    let fields = values.iter().map(|(key, value)| format!("{key}={value}"));
    (
        const_name,
        format!(
            "covopt.schema={};kind={kind};{}",
            covopt_schema::SCHEMA_VERSION,
            fields.collect::<Vec<_>>().join(";")
        ),
    )
}

fn metadata_string(value: &str) -> String {
    value.trim().trim_matches('"').to_string()
}

fn validate_contract_id(value: &str, label: &str) -> syn::Result<()> {
    let value = metadata_string(value);
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '.'))
    {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("invalid {label}"),
        ));
    }
    Ok(())
}

/// Declares the function-level contract that CovOpt checks.
#[proc_macro_attribute]
pub fn covopt_target(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = match syn::parse::<ItemFn>(item) {
        Ok(value) => value,
        Err(error) => return error.to_compile_error().into(),
    };
    let mut values = match parse_string_metadata(
        attr,
        "covopt_target",
        &["id", "complexity", "criticality", "scope"],
    ) {
        Ok(value) => value,
        Err(error) => return error.to_compile_error().into(),
    };
    values
        .entry("id".to_string())
        .or_insert_with(|| input_fn.sig.ident.to_string());
    if let Some(id) = values.get("id")
        && let Err(error) = validate_contract_id(id, "covopt target ID")
    {
        return error.to_compile_error().into();
    }
    if let Some(complexity) = values.get("complexity")
        && metadata_string(complexity).is_empty()
    {
        return syn::Error::new_spanned(
            &input_fn.sig.ident,
            "covopt target complexity must not be empty",
        )
        .to_compile_error()
        .into();
    }
    if let Some(criticality) = values.get("criticality")
        && !matches!(
            metadata_string(criticality).as_str(),
            "normal" | "high" | "critical"
        )
    {
        return syn::Error::new_spanned(
            &input_fn.sig.ident,
            "criticality must be `normal`, `high`, or `critical`",
        )
        .to_compile_error()
        .into();
    }
    let (metadata_name, metadata) = metadata_const(&input_fn.sig.ident, "target", &values);
    let vis = &input_fn.vis;
    quote! { #input_fn #[doc(hidden)] #[allow(dead_code, non_upper_case_globals)] #vis const #metadata_name: &str = #metadata; }.into()
}

/// Associates a test or benchmark with a declared CovOpt target.
#[proc_macro_attribute]
pub fn covopt_evidence(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = match syn::parse::<ItemFn>(item) {
        Ok(value) => value,
        Err(error) => return error.to_compile_error().into(),
    };
    let values = match parse_string_metadata(
        attr,
        "covopt_evidence",
        &[
            "target",
            "n",
            "n_values",
            "seeds",
            "seed",
            "threads",
            "environment",
            "axes",
        ],
    ) {
        Ok(value) => value,
        Err(error) => return error.to_compile_error().into(),
    };
    if !values.contains_key("target") {
        return syn::Error::new_spanned(&input_fn.sig.ident, "covopt_evidence requires `target`")
            .to_compile_error()
            .into();
    }
    if let Some(target) = values.get("target")
        && let Err(error) = validate_contract_id(target, "covopt evidence target ID")
    {
        return error.to_compile_error().into();
    }
    let (metadata_name, metadata) = metadata_const(&input_fn.sig.ident, "evidence", &values);
    let vis = &input_fn.vis;
    quote! { #input_fn #[doc(hidden)] #[allow(dead_code, non_upper_case_globals)] #vis const #metadata_name: &str = #metadata; }.into()
}

/// Marks an atomic target and records its correctness-contract metadata for
/// CovOpt tooling. The attribute is intentionally transparent at compile time;
/// `covopt atomic` performs the opt-in check and bounded analysis.
#[proc_macro_attribute]
pub fn covopt_atomic(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = match syn::parse::<ItemFn>(item) {
        Ok(value) => value,
        Err(error) => return error.to_compile_error().into(),
    };
    let values = match parse_string_metadata(
        attr,
        "covopt_atomic",
        &[
            "target",
            "ordering",
            "liveness",
            "forbidden_outcomes",
            "bounds",
        ],
    ) {
        Ok(value) => value,
        Err(error) => return error.to_compile_error().into(),
    };
    if !values.contains_key("target") {
        return syn::Error::new_spanned(&input_fn.sig.ident, "covopt_atomic requires `target`")
            .to_compile_error()
            .into();
    }
    if let Some(target) = values.get("target")
        && let Err(error) = validate_contract_id(target, "covopt atomic target ID")
    {
        return error.to_compile_error().into();
    }
    let (metadata_name, metadata) = metadata_const(&input_fn.sig.ident, "atomic", &values);
    let vis = &input_fn.vis;
    quote! { #input_fn #[doc(hidden)] #[allow(dead_code, non_upper_case_globals)] #vis const #metadata_name: &str = #metadata; }.into()
}

struct QsbrRegistryInput {
    vis: syn::Visibility,
    ident: syn::Ident,
    node_type: syn::Path,
    register: syn::Path,
    unregister: Option<syn::Path>,
}

impl syn::parse::Parse for QsbrRegistryInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let vis: syn::Visibility = input.parse()?;
        input.parse::<syn::Token![struct]>()?;
        let ident: syn::Ident = input.parse()?;
        input.parse::<syn::Token![;]>()?;

        let mut node_type = None;
        let mut register = None;
        let mut unregister = None;

        while !input.is_empty() {
            let kw: syn::Ident = input.parse()?;
            input.parse::<syn::Token![=]>()?;
            let p: syn::Path = input.parse()?;
            input.parse::<syn::Token![;]>()?;

            if kw == "node_type" {
                node_type = Some(p);
            } else if kw == "register" {
                register = Some(p);
            } else if kw == "unregister" {
                unregister = Some(p);
            } else {
                return Err(syn::Error::new(
                    kw.span(),
                    "Unknown keyword, expected node_type, register, or unregister",
                ));
            }
        }

        let node_type = node_type.ok_or_else(|| input.error("Missing node_type"))?;
        let register = register.ok_or_else(|| input.error("Missing register"))?;

        Ok(QsbrRegistryInput {
            vis,
            ident,
            node_type,
            register,
            unregister,
        })
    }
}

/// Generates an automatic QSBR TLS registry and Guard.
#[proc_macro]
pub fn covopt_qsbr_registry(input: TokenStream) -> TokenStream {
    let parsed = syn::parse_macro_input!(input as QsbrRegistryInput);

    let vis = parsed.vis;
    let ident = parsed.ident;
    let node_type = parsed.node_type;
    let register = parsed.register;
    let Some(unregister) = parsed.unregister else {
        return syn::Error::new(
            ident.span(),
            "covopt_qsbr_registry requires an explicit unregister function",
        )
        .to_compile_error()
        .into();
    };
    let unregister_code = quote! {
        unsafe { #unregister(self.node); }
    };

    let tl_name = syn::Ident::new(&format!("__COVOPT_{}_GUARD", ident), ident.span());
    let wrapper_name = syn::Ident::new(&format!("__CovOptQsbrTlsWrapper_{}", ident), ident.span());

    let expanded = quote! {
        std::thread_local! {
            #[allow(non_upper_case_globals)]
            static #tl_name: #wrapper_name = #wrapper_name::new();
        }

        #[allow(non_camel_case_types)]
        struct #wrapper_name {
            node: *mut #node_type,
        }

        impl #wrapper_name {
            fn new() -> Self {
                let node = unsafe {
                    let layout = core::alloc::Layout::new::<#node_type>();
                    let ptr = std::alloc::alloc_zeroed(layout) as *mut #node_type;
                    if ptr.is_null() {
                        std::alloc::handle_alloc_error(layout);
                    }
                    core::ptr::write(ptr, #node_type::new());
                    #register(ptr);
                    ptr
                };
                Self { node }
            }
        }

        impl Drop for #wrapper_name {
            fn drop(&mut self) {
                #unregister_code
                unsafe {
                    let layout = core::alloc::Layout::new::<#node_type>();
                    std::alloc::dealloc(self.node as *mut u8, layout);
                }
            }
        }

        #vis struct #ident;

        impl #ident {
            #[inline(always)]
            pub fn pin() -> *mut #node_type {
                #tl_name.with(|g| g.node)
            }
        }
    };

    TokenStream::from(expanded)
}
mod hoist;

/// Invisible Static Hoisting for Aerospace-Grade No-Std Memory Layouts
#[proc_macro_attribute]
pub fn covopt_hoist(args: TokenStream, input: TokenStream) -> TokenStream {
    hoist::covopt_hoist_impl(args, input)
}

/// Marks a benchmark function and automatically prevents Dead Code Elimination (DCE).
///
/// This macro wraps the function body in a closure and passes its result to `std::hint::black_box()`.
/// It also serves as a static marker for `covopt inspect` to identify hot paths.
/// and skip analyzing irrelevant code.
#[proc_macro_attribute]
pub fn covopt_bench(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let values = match parse_string_metadata(attr, "covopt_bench", &["target", "anti_dce"]) {
        Ok(values) => values,
        Err(error) => return error.to_compile_error().into(),
    };

    let fn_name = &input_fn.sig.ident;

    let fn_vis = &input_fn.vis;
    let sig = &input_fn.sig;
    let attrs = &input_fn.attrs;
    let orig_body = &input_fn.block;
    let (metadata_name, metadata) = metadata_const(fn_name, "bench", &values);

    // Wrap the entire original body in std::hint::black_box to prevent DCE
    let expanded = quote! {
        #(#attrs)*
        #fn_vis #sig {
            let mut __covopt_bench_inner = || {
                #orig_body
            };
            std::hint::black_box(__covopt_bench_inner())
        }
        #[doc(hidden)]
        #[allow(dead_code, non_upper_case_globals)]
        #fn_vis const #metadata_name: &str = #metadata;
    };

    TokenStream::from(expanded)
}

/// Declares an evolution target for CovOpt 3.0.
/// 
/// Intercepts a trait or struct and defines the physical survival boundaries (Chaos DSL).
/// When `covopt evolve` scans this macro, it hands the metadata and structural constraints
/// over to the Core Evolutionary Engine for topological searching (Phase 1~4).
#[proc_macro_attribute]
pub fn covopt_evolve(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_item = match syn::parse::<syn::Item>(item.clone()) {
        Ok(value) => value,
        Err(error) => return error.to_compile_error().into(),
    };

    let (item_ident, vis) = match &input_item {
        syn::Item::Struct(s) => (&s.ident, &s.vis),
        syn::Item::Trait(t) => (&t.ident, &t.vis),
        syn::Item::Fn(f) => (&f.sig.ident, &f.vis),
        syn::Item::Enum(e) => (&e.ident, &e.vis),
        _ => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                "#[covopt_evolve] can only be applied to a struct, trait, enum, or function",
            )
            .to_compile_error()
            .into();
        }
    };

    let mut values = match parse_string_metadata(
        attr,
        "covopt_evolve",
        &["bounds", "fuzzer", "target"],
    ) {
        Ok(value) => value,
        Err(error) => return error.to_compile_error().into(),
    };

    values
        .entry("target".to_string())
        .or_insert_with(|| item_ident.to_string());

    let (metadata_name, metadata) = metadata_const(item_ident, "evolve", &values);
    let orig_item: TokenStream2 = item.into();

    quote! { 
        #orig_item 
        
        #[doc(hidden)] 
        #[allow(dead_code, non_upper_case_globals)] 
        #vis const #metadata_name: &str = #metadata; 
    }.into()
}

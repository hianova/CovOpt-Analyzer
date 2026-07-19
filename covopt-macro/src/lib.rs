#![doc = include_str!("../README.md")]

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
#[macro_export]
macro_rules! covopt_param {
    ($name:expr, $default:expr) => {
        std::env::var(concat!("COVOPT_PARAM_", $name))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or($default)
    };
}

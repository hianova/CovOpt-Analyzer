//! The macroscopic Gene Pool for Rust AST structures.
//! 
//! Provides perfect, verified AST templates for concurrency and data structures.
//! Instead of mutating tokens randomly, the Punnett Square Combinator pulls from these genes.

use syn::{parse_quote, ItemStruct};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConcurrencyGene {
    Mutex,
    RwLock,
    LockFreeQueue,
    ActorModel,
    External(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum StorageGene {
    HashMap,
    BTreeMap,
    Vec,
    Slab,
    External(String),
}

pub struct GeneLibrary;

impl GeneLibrary {
    /// Returns the AST template for a Mutex wrapper.
    pub fn get_mutex_wrapper(name: &syn::Ident, inner_type: &syn::Type) -> ItemStruct {
        parse_quote! {
            pub struct #name {
                inner: std::sync::Mutex<#inner_type>,
            }
        }
    }

    /// Returns the AST template for an RwLock wrapper.
    pub fn get_rwlock_wrapper(name: &syn::Ident, inner_type: &syn::Type) -> ItemStruct {
        parse_quote! {
            pub struct #name {
                inner: std::sync::RwLock<#inner_type>,
            }
        }
    }

    /// Returns the AST template for a Lock-Free Queue.
    pub fn get_lockfree_queue(name: &syn::Ident, inner_type: &syn::Type) -> ItemStruct {
        parse_quote! {
            pub struct #name {
                queue: crossbeam::queue::SegQueue<#inner_type>,
            }
        }
    }

    /// Returns an AST template for an external plugin wrapper.
    pub fn get_external_wrapper(name: &syn::Ident, inner_type: &syn::Type, external_path: &str) -> ItemStruct {
        let external_type: syn::Type = syn::parse_str(external_path).unwrap_or_else(|_| parse_quote!(std::sync::Mutex));
        parse_quote! {
            pub struct #name {
                inner: #external_type<#inner_type>,
            }
        }
    }
}

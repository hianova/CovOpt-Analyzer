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
    TokioMutex,
    TokioRwLock,
    CrossbeamQueue,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum StorageGene {
    HashMap,
    BTreeMap,
    Vec,
    Slab,
    DashMap,
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
}

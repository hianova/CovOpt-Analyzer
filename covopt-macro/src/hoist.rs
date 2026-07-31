use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::visit::Visit;
use syn::{Error, ItemStruct, Lit, parse_macro_input};

struct AllocVisitor {
    errors: Vec<Error>,
}

impl<'ast> Visit<'ast> for AllocVisitor {
    fn visit_ident(&mut self, i: &'ast syn::Ident) {
        let name = i.to_string();
        if name == "alloc"
            || name == "Box"
            || name == "Vec"
            || name == "String"
            || name == "Rc"
            || name == "Arc"
        {
            self.errors.push(Error::new_spanned(i, format!("Forbidden token '{}' found! Allocation is not allowed in covopt_hoist structs.", name)));
        }
        syn::visit::visit_ident(self, i);
    }
}

pub fn covopt_hoist_impl(args: TokenStream, input: TokenStream) -> TokenStream {
    let input_ast = parse_macro_input!(input as ItemStruct);
    let mut visitor = AllocVisitor { errors: Vec::new() };
    visitor.visit_item_struct(&input_ast);
    if let Some(first_err) = visitor.errors.into_iter().next() {
        return first_err.to_compile_error().into();
    }
    let mut capacity: Option<usize> = None;
    let mut partition: Option<String> = None;
    let meta_parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("capacity") {
            let value: Lit = meta.value()?.parse()?;
            if let Lit::Int(int_lit) = value {
                capacity = Some(int_lit.base10_parse::<usize>()?);
            }
            Ok(())
        } else if meta.path.is_ident("partition") {
            let value: Lit = meta.value()?.parse()?;
            if let Lit::Str(str_lit) = value {
                partition = Some(str_lit.value());
            }
            Ok(())
        } else {
            Err(meta.error("unsupported covopt_hoist property"))
        }
    });
    parse_macro_input!(args with meta_parser);
    let capacity = match capacity {
        Some(c) => c,
        None => {
            return Error::new_spanned(&input_ast.ident, "Missing `capacity` attribute")
                .to_compile_error()
                .into();
        }
    };
    let partition = partition.unwrap_or_else(|| "default_pool".to_string());
    let section_name = format!(".bss.{}", partition);
    let struct_name = &input_ast.ident;
    let pool_name = format_ident!("{}_POOL", struct_name.to_string().to_uppercase());
    let bitmap_name = format_ident!("{}_BITMAP", struct_name.to_string().to_uppercase());
    let ready_name = format_ident!("{}_READY", struct_name.to_string().to_uppercase());
    let camel_partition: String = partition
        .split('_')
        .map(|word| {
            let mut c = word.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect();
    let token_name = format_ident!("{}Token", camel_partition);
    let num_bitmap_words = capacity.div_ceil(64);
    let stack_size = quote! {
        covopt_macro::covopt_param!("covopt_hoist::stack_bytes", 1024 * 1024 * 1024, class = "budget", unit = "bytes")
    };
    let expanded = quote! {
        #input_ast
        #[cfg_attr(target_vendor = "apple", unsafe(link_section = concat!("__DATA,", #partition)))]
        #[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = #section_name))]
        static mut #pool_name: [core::mem::MaybeUninit<#struct_name>; #capacity] = [const { core::mem::MaybeUninit::uninit() }; #capacity];
        #[cfg_attr(target_vendor = "apple", unsafe(link_section = concat!("__DATA,", #partition)))]
        #[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = #section_name))]
        static #bitmap_name: [core::sync::atomic::AtomicU64; #num_bitmap_words] =
            [const { core::sync::atomic::AtomicU64::new(0) }; #num_bitmap_words];
        static #ready_name: [core::sync::atomic::AtomicU64; #num_bitmap_words] =
            [const { core::sync::atomic::AtomicU64::new(0) }; #num_bitmap_words];
        #[derive(Default)]
        pub struct #token_name;
        impl #struct_name {
            pub unsafe fn insert(val: Self, _token: &mut #token_name) -> Option<usize> {
                unsafe {
                    for i in 0..#num_bitmap_words {
                        loop {
                            let word = #bitmap_name[i].load(core::sync::atomic::Ordering::Acquire);
                            if word == u64::MAX {
                                break;
                            }
                            let free_bit = (!word).trailing_zeros() as usize;
                            let slot_idx = i * 64 + free_bit;
                            if slot_idx >= #capacity {
                                break;
                            }
                            let claimed = word | (1u64 << free_bit);
                            if #bitmap_name[i].compare_exchange(
                                word,
                                claimed,
                                core::sync::atomic::Ordering::AcqRel,
                                core::sync::atomic::Ordering::Acquire,
                            ).is_ok() {
                                #pool_name[slot_idx].as_mut_ptr().write(val);
                                #ready_name[i].fetch_or(
                                    1u64 << free_bit,
                                    core::sync::atomic::Ordering::Release,
                                );
                                return Some(slot_idx);
                            }
                        }
                    }
                }
                None
            }
            #[cfg(feature = "std")]
            pub unsafe fn insert_large_std<F>(init_fn: F, _token: &mut #token_name) -> Option<usize>
            where F: FnOnce() -> Self + Send,
            {
                std::thread::scope(|s| {
                    let handle = std::thread::Builder::new()
                        .stack_size(#stack_size)
                        .spawn_scoped(s, || unsafe { Self::insert(init_fn(), _token) })
                        .ok()?;
                    handle.join().ok()?
                })
            }
            pub unsafe fn remove(index: usize, _token: &mut #token_name) -> Option<Self> {
                if index >= #capacity { return None; }
                let word_idx = index / 64;
                let bit_idx = index % 64;
                unsafe {
                    let word = #ready_name[word_idx].load(core::sync::atomic::Ordering::Acquire);
                    if (word & (1 << bit_idx)) != 0 {
                        #ready_name[word_idx].fetch_and(
                            !(1 << bit_idx),
                            core::sync::atomic::Ordering::AcqRel,
                        );
                        #bitmap_name[word_idx].fetch_and(
                            !(1 << bit_idx),
                            core::sync::atomic::Ordering::AcqRel,
                        );
                        let val = #pool_name[index].as_ptr().read();
                        Some(val)
                    } else { None }
                }
            }
            pub unsafe fn get(index: usize, _token: &#token_name) -> Option<&Self> {
                if index >= #capacity { return None; }
                let word_idx = index / 64;
                let bit_idx = index % 64;
                unsafe {
                    let word = #ready_name[word_idx].load(core::sync::atomic::Ordering::Acquire);
                    if (word & (1 << bit_idx)) != 0 { Some(&*#pool_name[index].as_ptr()) } else { None }
                }
            }
            pub unsafe fn get_mut(index: usize, _token: &mut #token_name) -> Option<&mut Self> {
                if index >= #capacity { return None; }
                let word_idx = index / 64;
                let bit_idx = index % 64;
                unsafe {
                    let word = #ready_name[word_idx].load(core::sync::atomic::Ordering::Acquire);
                    if (word & (1 << bit_idx)) != 0 { Some(&mut *#pool_name[index].as_mut_ptr()) } else { None }
                }
            }
        }
    };
    TokenStream::from(expanded)
}

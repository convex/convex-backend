use proc_macro::TokenStream;
use quote::{
    format_ident,
    quote,
};
use syn::{
    FnArg,
    GenericArgument,
    ItemFn,
    Pat,
    PathArguments,
    PathSegment,
    ReturnType,
    Safety,
    Signature,
    Type,
};

#[proc_macro_attribute]
pub fn instrument_future(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let ItemFn {
        attrs,
        vis,
        sig,
        block,
        modifiers,
    } = syn::parse(item).unwrap();

    assert!(sig.constness.is_none(), "Can't instrument const fn");
    assert!(sig.asyncness.is_some(), "Can only instrument async fn");
    assert!(
        !matches!(sig.safety, Safety::Unsafe(_)),
        "Can't instrument unsafe fn"
    );
    assert!(sig.abi.is_none(), "Can't instrument fn with explicit ABI");
    assert!(
        sig.variadic.is_none(),
        "Can't instrument fn with variadic arguments"
    );
    modifiers.require_empty().unwrap();

    let Signature {
        ident,
        generics,
        inputs,
        output,
        ..
    } = sig;

    let r#gen = quote! {
        #(#attrs)*
        #vis async fn #ident #generics (#inputs) #output {
            ::common::run_instrumented!(
                #ident,
                #block
            )
        }
    };
    r#gen.into()
}

/// Use as #[convex_macro::op] for an op whose only use of V8 is converting its
/// arguments and its result, which is nearly all of them.
///
/// Emits the function as written -- the one place the op's logic lives -- plus
/// a wrapper generic over `OpCall`, so the V8 and wasm dispatchers share it and
/// neither runtime's conversions are repeated per op.
///
/// The function must be generic over `P: OpProvider`, under that name. An op
/// that handles V8 values itself stays on [`macro@v8_op`].
#[proc_macro_attribute]
pub fn op(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let ast: ItemFn = syn::parse(item).unwrap();
    let ItemFn {
        ref attrs,
        ref vis,
        ref sig,
        ..
    } = ast;

    let Signature {
        ident,
        generics,
        inputs,
        ..
    } = sig;

    let Some(FnArg::Typed(first_pat_type)) = inputs.first() else {
        panic!("op should take a first argument for its op provider");
    };
    let Pat::Ident(first_pat_ident) = &*first_pat_type.pat else {
        panic!("op's first argument should be a plain identifier");
    };
    let provider_ident = &first_pat_ident.ident;

    let op_name = ident.to_string();
    let arg_idents: Vec<_> = (0..inputs.len() - 1)
        .map(|index| format_ident!("__arg{index}"))
        .collect();
    let arg_types = inputs.iter().skip(1).map(|input| {
        let FnArg::Typed(pat) = input else {
            panic!("input must be typed")
        };
        &*pat.ty
    });

    // Read as one tuple, not one call per argument: a V8 call opens a handle
    // scope to do it, and the op's own body needs the provider back afterwards.
    // An op with no arguments skips the scope entirely.
    let read_args = if arg_idents.is_empty() {
        quote! {}
    } else {
        quote! {
            let ( #(#arg_idents,)* ) = crate::ops::call::OpCall::args::<
                ( #(#arg_types,)* ),
            >(&mut __call, #provider_ident, #op_name)?;
        }
    };

    let call_name = format_ident!("{ident}_call");
    let (_, _, where_clause) = generics.split_for_impl();
    let generic_params = generics.params.iter();

    let r#gen = quote! {
        #ast

        #(#attrs)*
        #vis fn #call_name <
            #(#generic_params,)*
            __C: crate::ops::call::OpCall<P>,
        >(
            #first_pat_type,
            mut __call: __C,
        ) -> ::anyhow::Result<__C::Output> #where_clause {
            #read_args
            #[allow(clippy::unused_unit)]
            let __result = #ident(#provider_ident, #(#arg_idents,)*)?;
            crate::ops::call::OpCall::finish(__call, #provider_ident, __result)
        }
    };
    r#gen.into()
}

/// Use as #[convex_macro::v8_op] to annotate "ops" (Rust code callable from
/// JavaScript that is shipped with backend).
/// Must be used within the `isolate` crate.
///
/// Types:
/// Arguments and return value can be anything that implements
/// `serde::Serialize`. TODO: support &str and &mut [u8].
///
/// Note: Option::None in return values is encoded as `null` (not
/// undefined), while both `null` and `undefined` (and missing positional)
/// arguments become None.
///
/// The function should be called as `op_name(provider, args, rt)?`.
#[proc_macro_attribute]
pub fn v8_op(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let ItemFn {
        attrs,
        vis,
        sig,
        block,
        modifiers,
    } = syn::parse(item).unwrap();

    assert!(sig.constness.is_none(), "const fn cannot be op");
    assert!(sig.asyncness.is_none(), "async fn cannot be op");
    assert!(
        !matches!(sig.safety, Safety::Unsafe(_)),
        "unsafe fn cannot be op"
    );
    assert!(sig.abi.is_none(), "fn with explicit ABI cannot be op");
    assert!(
        sig.variadic.is_none(),
        "fn with variadic arguments cannot be op"
    );
    modifiers.require_empty().unwrap();

    let Signature {
        ident,
        generics,
        inputs,
        output,
        ..
    } = sig;

    let Some(FnArg::Typed(first_pat_type)) = inputs.first() else {
        panic!("op should take a first argument for its op provider");
    };
    let Pat::Ident(first_pat_ident) = &*first_pat_type.pat else {
        panic!("op's first argument should be a plain identifier");
    };
    let provider_ident = &first_pat_ident.ident;

    let arg_pats: Vec<_> = inputs
        .iter()
        .skip(1)
        .map(|input| {
            let FnArg::Typed(pat) = input else {
                panic!("input must be typed")
            };
            &pat.pat
        })
        .collect();
    let arg_parsing: Vec<_> = inputs
        .iter()
        .enumerate()
        .skip(1)
        .map(|(idx, input)| {
            let idx = idx as i32;
            let arg_info = format!("{ident} arg{idx}");
            let FnArg::Typed(pat) = input else {
                panic!("input must be typed")
            };
            let ty = &pat.ty;
            // NOTE: deno has special case when pat.ty is &mut [u8].
            // While that would make some ops more efficient, it also makes them
            // unsafe because it's hard to prove that the same buffer isn't
            // being mutated from multiple ops in parallel or multiple arguments
            // on the same op.
            //
            // Forego all special casing and just use serde_v8.
            quote! {
                {
                    let __raw_arg = __args.get(#idx);
                    use ::anyhow::Context as _;
                    <#ty as crate::convert_v8::FromV8>::from_v8(__scope, __raw_arg)
                        .context(#arg_info)?
                }
            }
        })
        .collect();

    let ReturnType::Type(_, return_type) = &output else {
        panic!("op needs return type");
    };
    let Type::Path(rtype_path) = &**return_type else {
        panic!("op must return anyhow::Result<...>")
    };
    let PathSegment {
        ident: retval_type,
        arguments: retval_arguments,
    } = rtype_path.path.segments.last().unwrap();
    assert_eq!(&retval_type.to_string(), "Result");
    let PathArguments::AngleBracketed(retval_arguments) = retval_arguments else {
        panic!("op must return anyhow::Result<...>")
    };
    let GenericArgument::Type(_retval_type) = retval_arguments
        .args
        .last()
        .expect("op must return anyhow::Result<...>")
    else {
        panic!("op must return anyhow::Result<...>");
    };

    let r#gen = quote! {
        #(#attrs)*
        #vis fn #ident #generics (
            #first_pat_type,
            __args: ::deno_core::v8::FunctionCallbackArguments,
            mut __rv: ::deno_core::v8::ReturnValue,
        ) -> ::anyhow::Result<()> {
            #[allow(clippy::unused_unit)]
            let ( #(#arg_pats,)*) = {
                let mut __scope = crate::ops::V8OpProvider::scope(#provider_ident);
                ::deno_core::v8::scope!(let __scope, &mut __scope);
                (#(#arg_parsing,)*)
            };
            let __result_v = (|| #output { #block })()?;
            {
                let mut __scope = crate::ops::V8OpProvider::scope(#provider_ident);
                ::deno_core::v8::scope!(let __scope, &mut __scope);
                let __value_v8 = crate::convert_v8::ToV8::to_v8(__result_v, __scope)?;
                __rv.set(__value_v8);
            }
            Ok(())
        }
    };
    r#gen.into()
}

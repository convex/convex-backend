//! An [`OpCall`] converts op arguments from the form the runtime gives it to
//! the form an Op accepts and converts the return values back to the form the
//! runtime accepts. V8Call uses v8 types, and wasm uses JSON since the data is
//! passed over the wasm boundary as text.

use anyhow::Context as _;
use deno_core::{
    serde_v8,
    v8,
};
use errors::ErrorMetadata;
use serde::{
    de::DeserializeOwned,
    Serialize,
};
use serde_json::value::RawValue;

use super::V8OpProvider;

/// Where one op's arguments are read from, positionally.
///
/// Only ever used from inside a [`OpCall::args`] implementation, which is what
/// lets the V8 one hold a handle scope: the scope is a local of that method, so
/// it never has to be named in a signature.
pub trait ArgSource {
    fn value<T: DeserializeOwned>(&mut self, op: &'static str, index: usize) -> anyhow::Result<T>;
}

/// An op's arguments as a whole, so that reading them costs one handle scope
/// rather than one per argument.
pub trait OpArgs: Sized {
    fn read<S: ArgSource>(source: &mut S, op: &'static str) -> anyhow::Result<Self>;
}

macro_rules! op_args_tuples {
    ($(($($index:tt $arg:ident),*),)*) => {
        $(
            impl<$($arg: DeserializeOwned,)*> OpArgs for ($($arg,)*) {
                fn read<S: ArgSource>(source: &mut S, op: &'static str) -> anyhow::Result<Self> {
                    let _ = (&source, op);
                    Ok(($(source.value::<$arg>(op, $index)?,)*))
                }
            }
        )*
    };
}

// Up to the widest op in the table, `crypto/subtle/importKey` and friends at
// five.
op_args_tuples! {
    (),
    (0 A0),
    (0 A0, 1 A1),
    (0 A0, 1 A1, 2 A2),
    (0 A0, 1 A1, 2 A2, 3 A3),
    (0 A0, 1 A1, 2 A2, 3 A3, 4 A4),
    (0 A0, 1 A1, 2 A2, 3 A3, 4 A4, 5 A5),
}

pub trait OpCall<P> {
    /// What the caller gets back. V8 writes through a `v8::ReturnValue` and has
    /// nothing left to hand over; a JSON caller takes the value itself.
    type Output;

    fn args<A: OpArgs>(&mut self, provider: &mut P, op: &'static str) -> anyhow::Result<A>;

    fn finish<T: Serialize>(self, provider: &mut P, value: T) -> anyhow::Result<Self::Output>;
}

fn invalid_argument(
    op: &'static str,
    index: usize,
    error: impl std::fmt::Display,
) -> anyhow::Error {
    ErrorMetadata::bad_request("InvalidArgument", format!("{op} arg{}: {error}", index + 1)).into()
}

/// Reads arguments off the V8 callback and writes the result back through it.
pub struct V8Call<'s> {
    args: v8::FunctionCallbackArguments<'s>,
    rv: v8::ReturnValue<'s>,
}

impl<'s> V8Call<'s> {
    pub fn new(args: v8::FunctionCallbackArguments<'s>, rv: v8::ReturnValue<'s>) -> Self {
        Self { args, rv }
    }
}

struct V8ArgSource<'a, 'b, 's, 'i> {
    scope: &'a mut v8::PinScope<'s, 'i>,
    args: &'b v8::FunctionCallbackArguments<'s>,
}

impl ArgSource for V8ArgSource<'_, '_, '_, '_> {
    fn value<T: DeserializeOwned>(&mut self, op: &'static str, index: usize) -> anyhow::Result<T> {
        // Past the op name, which the dispatcher has already read.
        let raw = self.args.get(index as i32 + 1);
        serde_v8::from_v8(self.scope, raw).map_err(|error| invalid_argument(op, index, error))
    }
}

impl<'b, P: V8OpProvider<'b>> OpCall<P> for V8Call<'_> {
    type Output = ();

    fn args<A: OpArgs>(&mut self, provider: &mut P, op: &'static str) -> anyhow::Result<A> {
        // One scope for every argument. The op's own body needs the provider
        // back, so the scope cannot outlive this call -- which is why the
        // arguments are read together rather than one call at a time.
        let mut scope = provider.scope();
        v8::scope!(let scope, &mut scope);
        A::read(
            &mut V8ArgSource {
                scope,
                args: &self.args,
            },
            op,
        )
    }

    fn finish<T: Serialize>(mut self, provider: &mut P, value: T) -> anyhow::Result<()> {
        let mut scope = provider.scope();
        v8::scope!(let scope, &mut scope);
        let value = serde_v8::to_v8(scope, value)?;
        self.rv.set(value);
        Ok(())
    }
}

/// A call from the wasm runtime, whose arguments arrive as the JSON text
/// `performOp` sends and whose result leaves the same way.
///
/// The arguments are split without being parsed: an op reads only the ones it
/// takes, and reads each straight from its own slice of the original text
/// rather than from a `Value` built out of it first.
pub struct WasmCall<'a> {
    args: Vec<&'a RawValue>,
}

impl<'a> WasmCall<'a> {
    pub fn new(args: &'a str) -> anyhow::Result<Self> {
        let args = serde_json::from_str(args).context("op args should be a JSON array")?;
        Ok(Self { args })
    }
}

impl ArgSource for WasmCall<'_> {
    fn value<T: DeserializeOwned>(&mut self, op: &'static str, index: usize) -> anyhow::Result<T> {
        // Absent reads as `null`, which is how `serde_v8` treats an argument
        // past the end of the V8 callback's list too.
        let raw = self.args.get(index).map_or("null", |arg| arg.get());
        serde_json::from_str(raw).map_err(|error| invalid_argument(op, index, error))
    }
}

impl<P> OpCall<P> for WasmCall<'_> {
    type Output = String;

    fn args<A: OpArgs>(&mut self, _provider: &mut P, op: &'static str) -> anyhow::Result<A> {
        A::read(self, op)
    }

    fn finish<T: Serialize>(self, _provider: &mut P, value: T) -> anyhow::Result<String> {
        Ok(serde_json::to_string(&value)?)
    }
}

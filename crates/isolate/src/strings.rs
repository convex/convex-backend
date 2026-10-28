use deno_core::v8;

pub struct StaticString {
    // V8 supports preallocating "one byte constant" strings: These strings must be ASCII and,
    // therefore, one byte UTF8.
    v8: v8::OneByteConst,
    string: &'static str,
}

impl StaticString {
    const fn new(string: &'static str) -> Self {
        Self {
            v8: v8::String::create_external_onebyte_const(string.as_bytes()),
            string,
        }
    }

    pub fn create<'s>(
        &'static self,
        scope: &v8::PinScope<'s, '_, ()>,
    ) -> anyhow::Result<v8::Local<'s, v8::String>> {
        v8::String::new_from_onebyte_const(scope, &self.v8)
            .ok_or_else(|| anyhow::anyhow!("Failed to create static string for {:?}", self.string))
    }

    pub fn rust_str(&self) -> &'static str {
        self.string
    }
}

macro_rules! declare_strings {
    ($s:ident $(,)?) => {
        #[allow(non_upper_case_globals)]
        pub(crate) const $s: StaticString = StaticString::new(stringify!($s));
    };

    ($name:ident => $s:expr $(,)?) => {
        #[allow(non_upper_case_globals)]
        pub(crate) const $name: StaticString = StaticString::new($s);
    };

    ($s:ident , $($rest:tt)*) => {
        declare_strings!($s);
        declare_strings!($($rest)*);
    };

    ($s:ident => $v:expr , $($rest:tt)*) => {
        declare_strings!($s => $v);
        declare_strings!($($rest)*);
    };
}

// Preallocate static strings that our runtime uses for interacting with
// userspace. You can add a bare identifier here, which will declare that
// identifier as a string, or explicitly name the string with the `$name =>
// $string` syntax.
declare_strings!(
    __frameData,
    _handler,
    _onInitCallbacks,
    Convex,
    asyncOp,
    asyncSyscall,
    data,
    default,
    DOMException,
    dynamic_import_unsupported => "dynamic module import unsupported",
    empty => "",
    experimental_reuseContext,
    export,
    exportArgs,
    exportReturns,
    import_meta_unsupported => "import.meta unsupported",
    internal_error => "Convex encountered an internal error",
    invokeAction,
    invokeMutation,
    invokeQuery,
    isAction,
    isInternal,
    isMutation,
    isPublic,
    isQuery,
    isRouter,
    lookup,
    getContinuationPreservedEmbedderData,
    op,
    path,
    runRequest,
    setContinuationPreservedEmbedderData,
    setup,
    setupTemporal,
    stack,
    syscall,
    TextDecoder,

    // crypto
    CryptoKey,
    publicKey,
    privateKey,
    public,
    r#type => "type",
    extractable,
    algorithm,
    usages,
    name,
);

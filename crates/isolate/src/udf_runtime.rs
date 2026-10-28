use std::{
    borrow::Cow,
    sync::OnceLock,
};

use anyhow::Context as _;
use common::knobs::ISOLATE_MAX_HEAP_EXTRA_SIZE;
use deno_core::{
    v8::{
        self,
        scope,
        MapFnTo,
    },
    ModuleSpecifier,
};

use crate::{
    bundled_js::system_udf_file,
    helpers,
    isolate::SETUP_URL,
    strings,
};

static BASE_SNAPSHOT: OnceLock<Vec<u8>> = OnceLock::new();

/// Creates a snapshot containing the UDF runtime in its default context and
/// saves it.
///
/// This must be called once per process, prior to calling
/// `create_isolate_with_udf_runtime`.
pub(crate) fn initialize() {
    let snapshot = create_base_snapshot().to_vec();
    BASE_SNAPSHOT
        .set(snapshot)
        .expect("can't initialize more than once");
}

/// Set a 64KB initial heap size
const INITIAL_HEAP_SIZE: usize = 1 << 16;

/// Creates a new V8 isolate from the saved snapshot. Contexts created in this
/// isolate will have the UDF runtime already loaded.
pub(crate) fn create_isolate_with_udf_runtime(
    create_params: v8::CreateParams,
    max_heap_size: usize,
) -> v8::OwnedIsolate {
    let snapshot = BASE_SNAPSHOT
        .get()
        .expect("udf_runtime::initialize not called");
    v8::Isolate::new(
        create_params
            .heap_limits(
                INITIAL_HEAP_SIZE,
                max_heap_size + *ISOLATE_MAX_HEAP_EXTRA_SIZE,
            )
            .snapshot_blob(Cow::Borrowed(&snapshot[..]).into())
            .external_references(external_references().into()),
    )
}

fn illegal_constructor<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    if let Some(msg) = v8::String::new(scope, "Illegal constructor") {
        let exception = v8::Exception::type_error(scope, msg);
        rv.set(scope.throw_exception(exception));
    }
}

fn external_references() -> Vec<v8::ExternalReference> {
    // TODO: make sure that everything is included in the list of external
    // references
    vec![v8::ExternalReference {
        function: illegal_constructor.map_fn_to(),
    }]
}

fn create_base_snapshot() -> v8::StartupData {
    // TODO: set external references. For now we:
    // 1. do not reuse snapshot blobs across processes,
    // 2. only use 'static external references
    // so this is OK.
    let mut isolate = v8::Isolate::snapshot_creator(Some(Cow::Owned(external_references())), None);

    {
        scope!(let scope, &mut isolate);
        let context = v8::Context::new(scope, v8::ContextOptions::default());

        let crypto_key = v8::FunctionTemplate::new(scope, illegal_constructor);
        crypto_key.set_class_name(strings::CryptoKey.create(scope).unwrap());
        assert!(crypto_key
            .instance_template(scope)
            .set_internal_field_count(1));
        let crypto_key_prototype = crypto_key.prototype_template(scope);
        let symbol_tostringtag = v8::Symbol::get_to_string_tag(scope);
        crypto_key_prototype.set_with_attr(
            symbol_tostringtag.into(),
            strings::CryptoKey.create(scope).unwrap().into(),
            v8::PropertyAttribute::DONT_ENUM | v8::PropertyAttribute::READ_ONLY,
        );

        let crypto_key_private = v8::ObjectTemplate::new(scope);
        assert!(crypto_key_private.set_internal_field_count(1));

        let text_decoder = v8::FunctionTemplate::new(scope, illegal_constructor);
        text_decoder.set_class_name(strings::TextDecoder.create(scope).unwrap());
        assert!(text_decoder
            .instance_template(scope)
            .set_internal_field_count(1));

        let text_decoder_private = v8::ObjectTemplate::new(scope);
        assert!(text_decoder_private.set_internal_field_count(1));

        let context_scope = &mut v8::ContextScope::new(scope, context);

        // Create `global.Convex`, so that `setup.js` can populate `Convex.jsSyscall`
        let convex_value = v8::Object::new(context_scope);
        let convex_key = strings::Convex.create(context_scope).unwrap();
        let global = context.global(context_scope);
        global.set(context_scope, convex_key.into(), convex_value.into());

        {
            let extras_binding = context.get_extras_binding_object(context_scope);

            let get_cped_key = strings::getContinuationPreservedEmbedderData
                .create(context_scope)
                .unwrap();
            let get_cped_fn: v8::Local<v8::Value> = extras_binding
                .get(context_scope, get_cped_key.into())
                .expect("Missing getContinuationPreservedEmbedderData extras binding");
            convex_value.set(context_scope, get_cped_key.into(), get_cped_fn);

            let set_cped_key = strings::setContinuationPreservedEmbedderData
                .create(context_scope)
                .unwrap();
            let set_cped_fn: v8::Local<v8::Value> = extras_binding
                .get(context_scope, set_cped_key.into())
                .expect("Missing setContinuationPreservedEmbedderData extras binding");
            convex_value.set(context_scope, set_cped_key.into(), set_cped_fn);
        }

        {
            let crypto_key_instance = crypto_key
                .get_function(context_scope)
                .expect("instantiate CryptoKey");
            let crypto_key_key = strings::CryptoKey.create(context_scope).unwrap();
            global.set(
                context_scope,
                crypto_key_key.into(),
                crypto_key_instance.into(),
            );
            // Stash the reference to CryptoKey in case `global.CryptoKey` is overwritten
            let private_crypto_key_key = v8::Private::for_api(context_scope, Some(crypto_key_key));
            let crypto_key_private_instance = crypto_key_private
                .new_instance(context_scope)
                .expect("instantiate CryptoKeyPrivate");
            assert!(crypto_key_private_instance.set_internal_field(0, crypto_key.into()));
            global.set_private(
                context_scope,
                private_crypto_key_key,
                crypto_key_private_instance.into(),
            );
        }

        {
            // The TextDecoder template is only stashed in a private, not
            // exported as a global, since `08_text_encoding.ts` defines the
            // public TextDecoder class itself.
            let text_decoder_key = strings::TextDecoder.create(context_scope).unwrap();
            let private_text_decoder_key =
                v8::Private::for_api(context_scope, Some(text_decoder_key));
            let text_decoder_private_instance = text_decoder_private
                .new_instance(context_scope)
                .expect("instantiate TextDecoderPrivate");
            assert!(text_decoder_private_instance.set_internal_field(0, text_decoder.into()));
            global.set_private(
                context_scope,
                private_text_decoder_key,
                text_decoder_private_instance.into(),
            );
        }

        run_setup_module(context_scope).expect("failed to run setup module");

        // Mark the context we created as the "default context", so that every
        // new context created from the snapshot will include this
        // runtime.
        context_scope.set_default_context(context);
    }

    isolate
        .create_blob(v8::FunctionCodeHandling::Keep)
        .expect("Failed to create snapshot")
}

/// Go through all the V8 boilerplate to compile, instantiate, evaluate, and run
/// the setup code. This is all inlined to avoid any dependencies on context
/// state that isn't set up in the snapshot creation code path.
fn run_setup_module(scope: &mut v8::PinScope<'_, '_>) -> anyhow::Result<()> {
    let setup_url = ModuleSpecifier::parse(SETUP_URL)?;
    let (source, _source_map) = system_udf_file("setup.js").context("Setup module not found")?;
    let name_str =
        v8::String::new(scope, setup_url.as_str()).context("Failed to create name_str")?;
    let source_str = v8::String::new(scope, source).context("Failed to create source_str")?;
    let origin = helpers::module_origin(scope, name_str);
    let mut v8_source = v8::script_compiler::Source::new(source_str, Some(&origin));
    let module = v8::script_compiler::compile_module(scope, &mut v8_source)
        .context("Failed to compile setup module")?;

    // setup.js is bundled into a single module and so it doesn't need to import
    // anything.
    fn noop_resolve_module<'a>(
        _context: v8::Local<'a, v8::Context>,
        _specifier: v8::Local<'a, v8::String>,
        _import_assertions: v8::Local<'a, v8::FixedArray>,
        _referrer: v8::Local<'a, v8::Module>,
    ) -> Option<v8::Local<'a, v8::Module>> {
        None
    }
    anyhow::ensure!(
        module.instantiate_module(scope, noop_resolve_module) == Some(true),
        "Failed to instantiate setup module"
    );
    let evaluation_result = module.evaluate(scope).context("evaluate returned None")?;
    let status = module.get_status();
    anyhow::ensure!(
        status == v8::ModuleStatus::Evaluated,
        "Evaluating setup module failed with status {:?}",
        status
    );
    let promise = v8::Local::<v8::Promise>::try_from(evaluation_result)
        .context("Setup module didn't evaluate to a promise")?;
    anyhow::ensure!(
        promise.state() == v8::PromiseState::Fulfilled,
        "Setup module promise failed with state {:?}",
        promise.state()
    );

    let namespace = module
        .get_module_namespace()
        .to_object(scope)
        .context("Module namespace wasn't an object?")?;

    let function_str = strings::setup.create(scope)?;
    let function: v8::Local<v8::Function> = namespace
        .get(scope, function_str.into())
        .context("Missing setup function")?
        .try_into()?;

    let global = scope.get_current_context().global(scope);
    function
        .call(scope, global.into(), &[global.into()])
        .context("calling setup failed")?;
    Ok(())
}

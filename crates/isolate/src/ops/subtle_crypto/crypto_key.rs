use std::rc::Rc;

use anyhow::Context as _;
use deno_core::v8;
use slab::Slab;
use webcrypto::{
    CryptoKey,
    CryptoKeyKind,
    CryptoKeyOrPair,
};

use crate::{
    convert_v8::{
        FromV8,
        ToV8,
        TypeError,
    },
    strings,
};

#[derive(Default)]
struct CryptoKeyStore {
    keys: Slab<(v8::Weak<v8::Object>, Rc<CryptoKey>)>,
}

/// Op argument/return type for a JS `CryptoKey` object.
pub(super) struct JsCryptoKey(pub CryptoKey);

fn get_crypto_key_template<'s>(
    scope: &mut v8::PinScope<'s, '_>,
) -> anyhow::Result<v8::Local<'s, v8::FunctionTemplate>> {
    let s = strings::CryptoKey.create(scope)?;
    let private = v8::Private::for_api(scope, Some(s));
    let obj: v8::Local<'_, v8::Object> = scope
        .get_current_context()
        .global(scope)
        .get_private(scope, private)
        .context("missing CryptoKey private")?
        .try_cast()?;
    let template: v8::Local<'_, v8::FunctionTemplate> = obj
        .get_internal_field(scope, 0)
        .context("internal field missing")?
        .try_cast()?;
    Ok(template)
}

// Looks up a Rust CryptoKey in the CryptoKeyStore based on the passed-in
// CryptoKey instance.
impl FromV8 for JsCryptoKey {
    type Output = Rc<CryptoKey>;

    fn from_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        input: v8::Local<'s, v8::Value>,
    ) -> anyhow::Result<Rc<CryptoKey>> {
        let crypto_key = get_crypto_key_template(scope)?;
        let crypto_key_constructor = crypto_key
            .get_function(scope)
            .context("get CryptoKey constructor")?;
        let input = if let Ok(input) = input.try_cast::<v8::Object>()
            && input.instance_of(scope, crypto_key_constructor.into()) == Some(true)
        {
            input
        } else {
            anyhow::bail!(TypeError::new("not of type CryptoKey"));
        };
        let (id, ok) = input
            .get_internal_field(scope, 0)
            .context("missing internal field")?
            .try_cast::<v8::BigInt>()?
            .u64_value();
        anyhow::ensure!(ok);
        let (weak, key) = scope
            .get_slot::<CryptoKeyStore>()
            .context("missing CryptoKeyStore")?
            .keys
            .get(id as usize)
            .context("dangling CryptoKey")?;
        anyhow::ensure!(*weak == input, "CryptoKey id reused");
        Ok(key.clone())
    }
}

// Converts a freshly created CryptoKey instance into a new JS object and
// records it in the CryptoKeyStore so that it can be passed back into crypto
// APIs.
//
// Note that we never need to return a pre-existing CryptoKey instance.
impl ToV8 for JsCryptoKey {
    fn to_v8<'s>(
        self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> anyhow::Result<v8::Local<'s, v8::Value>> {
        let key = self.0;
        let crypto_key = get_crypto_key_template(scope)?;
        let object = crypto_key
            .instance_template(scope)
            .new_instance(scope)
            .context("failed to create instance")?;
        anyhow::ensure!(object.internal_field_count() == 1);
        let type_str = strings::r#type.create(scope)?;
        let r#type = key.r#type.to_v8(scope)?;
        anyhow::ensure!(
            object.define_own_property(
                scope,
                type_str.into(),
                r#type,
                v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE
            ) == Some(true)
        );
        let extractable_str = strings::extractable.create(scope)?;
        let extractable = key.extractable.to_v8(scope)?;
        anyhow::ensure!(
            object.define_own_property(
                scope,
                extractable_str.into(),
                extractable,
                v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE
            ) == Some(true)
        );
        let algorithm_str = strings::algorithm.create(scope)?;
        // TODO: the resulting `algorithm` object has a null prototype, which
        // looks ugly when inspected.
        let algorithm = match &key.kind {
            CryptoKeyKind::Pbkdf2 { algorithm, .. } => algorithm.to_v8(scope)?,
            CryptoKeyKind::Hkdf { algorithm, .. } => algorithm.to_v8(scope)?,
            CryptoKeyKind::Hmac { algorithm, .. } => algorithm.to_v8(scope)?,
            CryptoKeyKind::Aes { algorithm, .. } => algorithm.to_v8(scope)?,
            CryptoKeyKind::RsaPrivate { algorithm, .. } => algorithm.to_v8(scope)?,
            CryptoKeyKind::RsaPublic { algorithm, .. } => algorithm.to_v8(scope)?,
            CryptoKeyKind::EcPrivate { algorithm, .. } => algorithm.to_v8(scope)?,
            CryptoKeyKind::EcPublic { algorithm, .. } => algorithm.to_v8(scope)?,
            CryptoKeyKind::Ed25519Private { algorithm, .. } => algorithm.to_v8(scope)?,
            CryptoKeyKind::Ed25519Public { algorithm, .. } => algorithm.to_v8(scope)?,
            CryptoKeyKind::X25519Private { algorithm, .. } => algorithm.to_v8(scope)?,
            CryptoKeyKind::X25519Public { algorithm, .. } => algorithm.to_v8(scope)?,
        };
        anyhow::ensure!(
            object.define_own_property(
                scope,
                algorithm_str.into(),
                algorithm,
                v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE
            ) == Some(true)
        );
        let usages_str = strings::usages.create(scope)?;
        let usages = (&key.usages).to_v8(scope)?;
        anyhow::ensure!(
            object.define_own_property(
                scope,
                usages_str.into(),
                usages,
                v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE
            ) == Some(true)
        );
        let mut store: CryptoKeyStore = scope.remove_slot().unwrap_or_default();
        let entry = store.keys.vacant_entry();
        let id = entry.key();
        assert!(object.set_internal_field(0, v8::BigInt::new_from_u64(scope, id as u64).into()));
        // Install a finalizer so that we garbage collect the Rust data together
        // with the JS object.
        // This does not need to be a "guaranteed finalizer" because the data is
        // also dropped when the Isolate is destroyed.
        let weak = v8::Weak::with_finalizer(
            scope,
            object,
            Box::new(move |isolate| {
                if let Some(store) = isolate.get_slot_mut::<CryptoKeyStore>() {
                    store.keys.remove(id);
                }
            }),
        );
        entry.insert((weak, Rc::new(key)));
        scope.set_slot(store);
        Ok(object.into())
    }
}

/// Op return type for a JS `{privateKey, publicKey}` pair or a single
/// `CryptoKey`.
pub(super) struct JsCryptoKeyOrPair(pub CryptoKeyOrPair);

impl ToV8 for JsCryptoKeyOrPair {
    fn to_v8<'s>(
        self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> anyhow::Result<v8::Local<'s, v8::Value>> {
        match self.0 {
            CryptoKeyOrPair::Symmetric(k) => JsCryptoKey(k).to_v8(scope),
            CryptoKeyOrPair::Asymmetric(pair) => {
                // N.B.: a CryptoKeyPair is just a regular object, not a special class
                let private_key = JsCryptoKey(pair.private_key).to_v8(scope)?;
                let public_key = JsCryptoKey(pair.public_key).to_v8(scope)?;
                let crypto_key_pair = v8::Object::new(scope);
                let private_key_str = strings::privateKey.create(scope)?;
                anyhow::ensure!(
                    crypto_key_pair.set(scope, private_key_str.into(), private_key) == Some(true)
                );
                let public_key_str = strings::publicKey.create(scope)?;
                anyhow::ensure!(
                    crypto_key_pair.set(scope, public_key_str.into(), public_key) == Some(true)
                );
                Ok(crypto_key_pair.into())
            },
        }
    }
}

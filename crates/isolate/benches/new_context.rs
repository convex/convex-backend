use deno_core::v8::{
    self,
    scope,
};
use runtime::prod::ProdRuntime;

fn main() {
    isolate::client::initialize_v8();
    divan::main();
}

#[divan::bench]
fn create_context(bencher: divan::Bencher) {
    let tokio_rt = tokio::runtime::Builder::new_multi_thread().build().unwrap();
    let rt = ProdRuntime::new(&tokio_rt);
    let mut isolate = isolate::isolate::Isolate::new(
        rt.clone(),
        None,
        *common::knobs::ISOLATE_MAX_USER_HEAP_SIZE,
    );
    bencher.bench_local(|| {
        scope!(let scope, isolate.isolate());
        v8::Context::new(scope, v8::ContextOptions::default());
    });
}

#[divan::bench]
fn create_isolate(bencher: divan::Bencher) {
    let tokio_rt = tokio::runtime::Builder::new_multi_thread().build().unwrap();
    let rt = ProdRuntime::new(&tokio_rt);
    bencher.bench(|| {
        let mut isolate = isolate::isolate::Isolate::new(
            rt.clone(),
            None,
            *common::knobs::ISOLATE_MAX_USER_HEAP_SIZE,
        );
        scope!(let scope, isolate.isolate());
        v8::Context::new(scope, v8::ContextOptions::default());
    });
}

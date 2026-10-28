#[derive(Default)]
pub enum Fault {
    #[default]
    Noop,
    Error(anyhow::Error),
}

mod prod_pause {
    use super::Fault;

    #[derive(Default, Clone)]
    pub struct PauseClient;

    impl PauseClient {
        pub fn new() -> Self {
            Self
        }

        pub async fn wait(&self, _label: &'static str) -> Fault {
            Fault::Noop
        }

        pub fn close(&self, _label: &'static str) {}
    }
}
pub use self::prod_pause::PauseClient;

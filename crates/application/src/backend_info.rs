use anyhow::Context;
use common::{
    runtime::Runtime,
    types::{
        BackendInfo,
        Timestamp,
        WriteTimestamp,
    },
};
use database::Transaction;
use keybroker::Identity;
use model::{
    backend_info::{
        types::BackendInfoPersisted,
        BackendInfoModel,
    },
    log_sinks::LogSinksModel,
};

use crate::Application;

impl<RT: Runtime> Application<RT> {
    pub async fn set_logging_tags(&self) -> anyhow::Result<()> {
        let _writer = self.backend_info_write_lock.lock().await;
        if let Some((backend_info, _)) = self.backend_info().await? {
            self.set_logging_tags_from_backend_info(&backend_info.into());
        }
        Ok(())
    }

    pub async fn backend_info(&self) -> anyhow::Result<Option<(BackendInfo, Timestamp)>> {
        let mut tx = self.begin(Identity::system()).await?;
        let Some(bi) = BackendInfoModel::new(&mut tx).get().await? else {
            return Ok(None);
        };
        let (_, ts) = tx
            .get_with_ts(bi.id())
            .await?
            .context("Backend info not found?")?;
        let WriteTimestamp::Committed(ts) = ts else {
            anyhow::bail!("Pending timestamp on backend info?");
        };
        Ok(Some(((**bi).clone().into(), ts)))
    }

    pub async fn write_backend_info(
        &self,
        identity: Identity,
        backend_info: BackendInfo,
    ) -> anyhow::Result<()> {
        let _writer = self.backend_info_write_lock.lock().await;
        let backend_info: BackendInfoPersisted = backend_info.into();
        let mut tx = self.begin(identity).await?;
        let existing = BackendInfoModel::new(&mut tx).get().await?;
        if existing.is_none_or(|doc| **doc != backend_info) {
            self.persist_backend_info(tx, &backend_info).await?;
        }
        self.apply_backend_info_runtime(&backend_info);
        Ok(())
    }

    /// Configures the log-streaming permission and Sentry tags from this
    /// deployment's BackendInfo, which may survive a reload or come
    /// from the pull worker. `initial` is inserted only when no document
    /// exists. Returns false when neither is available, so a caller can pass
    /// `None`, fetch a value on false, and call again with `Some`; that second
    /// call checks again under the lock, so a document written in the meantime
    /// wins over `initial`.
    pub async fn initialize_backend_info(
        &self,
        initial: Option<BackendInfo>,
    ) -> anyhow::Result<bool> {
        let _writer = self.backend_info_write_lock.lock().await;
        let mut tx = self.begin(Identity::system()).await?;
        let backend_info = match BackendInfoModel::new(&mut tx).get().await? {
            Some(doc) => (**doc).clone(),
            None => {
                let Some(initial) = initial else {
                    return Ok(false);
                };
                let backend_info = initial.into();
                self.persist_backend_info(tx, &backend_info).await?;
                backend_info
            },
        };
        self.apply_backend_info_runtime(&backend_info);
        Ok(true)
    }

    async fn persist_backend_info(
        &self,
        mut tx: Transaction<RT>,
        backend_info: &BackendInfoPersisted,
    ) -> anyhow::Result<()> {
        BackendInfoModel::new(&mut tx)
            .set(backend_info.clone())
            .await?;
        // Tombstone existing sinks so re-enabling them starts with a clean slate.
        if !backend_info.log_streaming_enabled {
            tracing::info!(
                "Clearing _log_sinks table due to log streaming entitlement being disabled"
            );
            LogSinksModel::new(&mut tx).clear().await?;
        }
        self.commit(tx, "write_backend_info").await?;
        Ok(())
    }

    fn apply_backend_info_runtime(&self, backend_info: &BackendInfoPersisted) {
        self.log_manager_client()
            .set_entitlement_enabled(backend_info.log_streaming_enabled);
        self.set_logging_tags_from_backend_info(backend_info);
    }

    fn set_logging_tags_from_backend_info(&self, backend_info: &BackendInfoPersisted) {
        *self.sentry_tags.write() = vec![
            ("project_id", backend_info.project.0.to_string()),
            ("team_id", backend_info.team.0.to_string()),
            ("deployment_id", backend_info.deployment.0.to_string()),
            ("instance_name", self.deployment_name()),
        ];
    }

    pub fn add_sentry_tags(&self, scope: &mut sentry::Scope) {
        for (name, value) in &*self.sentry_tags.read() {
            scope.set_tag(name, value);
        }
    }
}

use std::time::SystemTime;

use common::{
    document::InternalId,
    runtime::Runtime,
};
use rand::{
    Rng,
    RngCore,
    SeedableRng,
};
use rand_chacha::ChaCha12Rng;
use value::{
    DeveloperDocumentId,
    ResolvedDocumentId,
    TableNumber,
    TabletIdAndTableNumber,
};

/// A production ID generator scoped to a single transaction.
///
/// This creates InternalIds with 14 bytes of randomness followed by the day in
/// 2 bytes.
///
/// The time is pinned on construction, so only use for a single transaction!
pub struct TransactionIdGenerator {
    rng: ChaCha12Rng,
    day_bytes: [u8; 2],
}

impl TransactionIdGenerator {
    pub fn new<RT: Runtime>(runtime: &RT) -> anyhow::Result<Self> {
        let rng_seed = runtime.rng().random();
        let rng = ChaCha12Rng::from_seed(rng_seed);

        // Get the current day as 2 bytes.
        let duration = runtime
            .system_time()
            .duration_since(SystemTime::UNIX_EPOCH)?;
        let days = duration.as_secs() / 86400;
        let day_bytes = days.to_be_bytes();
        // First 6 bytes should always be 0 (this only works until 2149).
        anyhow::ensure!(day_bytes[..6] == [0u8; 6]);

        Ok(Self {
            rng,
            day_bytes: day_bytes[6..].try_into()?,
        })
    }

    pub fn generate_internal(&mut self) -> InternalId {
        let mut id_bytes = [0u8; 16];
        self.rng.fill_bytes(&mut id_bytes[..14]);
        id_bytes[14..].clone_from_slice(&self.day_bytes);
        InternalId(id_bytes)
    }

    pub fn generate(&mut self, table_number: TableNumber) -> DeveloperDocumentId {
        DeveloperDocumentId::new(table_number, self.generate_internal())
    }

    pub fn generate_resolved(
        &mut self,
        tablet_id_and_number: TabletIdAndTableNumber,
    ) -> ResolvedDocumentId {
        ResolvedDocumentId::new(
            tablet_id_and_number.tablet_id,
            DeveloperDocumentId::new(tablet_id_and_number.table_number, self.generate_internal()),
        )
    }

    /// Clone the TransactionIdGenerator to pass into the transaction for a
    /// snapshot query. The resulting generator should not be used.
    pub(crate) fn clone_for_snapshot_query(&self) -> Self {
        TransactionIdGenerator {
            rng: self.rng.clone(),
            day_bytes: self.day_bytes,
        }
    }
}

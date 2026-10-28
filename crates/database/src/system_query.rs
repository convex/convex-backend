//! Provides a simplified, more-typed API for querying system metadata tables
//! (as compared to [`common::query::Query`] / [`crate::query::ResolvedQuery`]).

use std::{
    mem,
    ops::{
        Bound,
        RangeBounds,
    },
    sync::Arc,
};

use anyhow::Context as _;
use common::{
    document::{
        ParseDocument as _,
        ParsedDocument,
    },
    interval::{
        BinaryKey,
        End,
        Interval,
        StartIncluded,
    },
    knobs::DEFAULT_QUERY_PREFETCH,
    query::Order,
    runtime::Runtime,
};
use indexing::{
    in_memory_indexes::LazyDocument,
    index_reader::RangeRequest,
};
use value::{
    serde::ConvexSerializable,
    sorting::write_sort_key,
    walk::ConvexValueWalker,
    DeveloperDocumentId,
    TableNamespace,
    TabletId,
};

use crate::{
    system_tables::{
        SystemIndex,
        SystemTable,
    },
    Transaction,
};

pub struct EqFields {
    prefix: Vec<u8>,
    fields: usize,
}
pub struct Unlimited;

pub struct SystemQueryBuilder<'a, 'b, RT: Runtime, T: SystemTable, R> {
    tx: &'a mut Transaction<RT>,
    namespace: TableNamespace,
    index: &'b SystemIndex<T>,
    tablet_id: TabletId,
    index_range: R,
    order: Order,
}

pub struct SystemQuery<'a, 'b, RT: Runtime, T: SystemTable> {
    tx: &'a mut Transaction<RT>,
    index: &'b SystemIndex<T>,
    tablet_id: TabletId,
    index_range: Interval,
    order: Order,
    queue: std::vec::IntoIter<Arc<ParsedDocument<T::Metadata>>>,
}

impl<RT: Runtime> Transaction<RT> {
    /// Start building a query over `index`.
    /// The query will initially range over the entire index. To narrow the
    /// range, use the `.eq()` and `.range()` builder methods.
    pub fn query_system<'a, 'b, T: SystemTable>(
        &'a mut self,
        namespace: TableNamespace,
        index: &'b SystemIndex<T>,
    ) -> anyhow::Result<SystemQueryBuilder<'a, 'b, RT, T, EqFields>>
    where
        T::Metadata: ConvexSerializable,
    {
        // TODO: this should possibly return an empty vector?
        let tablet_id = self
            .table_mapping()
            .namespace(namespace)
            .id_if_exists(&T::TABLE_NAME)
            .with_context(|| format!("Index {:?} not present in {namespace:?}", index.name()))?;
        Ok(SystemQueryBuilder {
            tx: self,
            namespace,
            index,
            tablet_id,
            index_range: EqFields {
                prefix: vec![],
                fields: 0,
            },
            order: Order::Asc,
        })
    }
}

impl<'a, 'b, RT: Runtime, T: SystemTable> SystemQueryBuilder<'a, 'b, RT, T, EqFields> {
    /// Specify equality constraints for the next fields in the index.
    /// This will return an error if the total number of `.eq()` fields exceeds
    /// the number of indexed fields.
    ///
    /// This is not type-safe with respect to the actual field types - providing
    /// an eq() constraint of the wrong type will find nothing.
    pub fn eq<F: ConvexValueWalker + Copy>(mut self, fields: &[F]) -> anyhow::Result<Self> {
        let indexed_fields_len = self.index.fields.len() + 1; // include _id
        self.index_range.push(fields, indexed_fields_len)?;
        Ok(self)
    }

    /// Apply a one- or two-sided inequality to the index range.
    ///
    /// The inequality is applied to the same number of index fields as the
    /// number of values provided. For example, if the next fields in the index
    /// are `["nextTs", "_creationTime", "_id"]`, then calling
    /// `query.range(..=[a, b])` ranges over `(nextTs, _creationTime) <= (a,
    /// b)` and ignores `_id`.
    pub fn range<F: ConvexValueWalker + Copy, A: AsRef<[F]>>(
        self,
        range: impl RangeBounds<A>,
    ) -> anyhow::Result<SystemQueryBuilder<'a, 'b, RT, T, Interval>> {
        let SystemQueryBuilder {
            tx,
            namespace,
            index,
            tablet_id,
            index_range,
            order,
        } = self;
        let indexed_fields_len = self.index.fields.len() + 1; // include _id
        let index_range = index_range.range(
            range.start_bound().map(AsRef::as_ref),
            range.end_bound().map(AsRef::as_ref),
            indexed_fields_len,
        )?;
        Ok(SystemQueryBuilder {
            tx,
            namespace,
            index,
            tablet_id,
            index_range,
            order,
        })
    }

    /// Sets the iteration order of the query. The query is ascending by
    /// default.
    pub fn order(mut self, order: Order) -> Self {
        self.order = order;
        self
    }
}

impl<'a, 'b, RT: Runtime, T: SystemTable, R: Into<Interval>> SystemQueryBuilder<'a, 'b, RT, T, R> {
    /// Builds the query so that it can be iterated with [`SystemQuery::next`].
    pub fn build(self) -> SystemQuery<'a, 'b, RT, T> {
        let Self {
            tx,
            namespace: _,
            index,
            tablet_id,
            index_range,
            order,
        } = self;
        SystemQuery {
            tx,
            index,
            tablet_id,
            index_range: index_range.into(),
            order,
            queue: vec![].into_iter(),
        }
    }

    /// Returns all the documents in the index range.
    pub async fn all(self) -> anyhow::Result<Vec<Arc<ParsedDocument<T::Metadata>>>>
    where
        T::Metadata: ConvexSerializable,
    {
        let mut q = self.build();
        let mut docs = vec![];
        loop {
            let (page, has_more) = q.next_page(*DEFAULT_QUERY_PREFETCH).await?;
            docs.extend(page);
            if !has_more {
                break;
            }
        }
        Ok(docs)
    }

    /// Returns the unique document, if any, in the range. Raises an error if
    /// more than one document is found.
    pub async fn unique(self) -> anyhow::Result<Option<Arc<ParsedDocument<T::Metadata>>>>
    where
        T::Metadata: ConvexSerializable,
    {
        let mut q = self.build();
        let (mut docs, _has_more) = q.next_page(2).await?;
        anyhow::ensure!(
            docs.len() <= 1,
            "expected at most 1 document from index {:?}, got {}",
            q.index.name(),
            docs.len()
        );
        Ok(docs.pop())
    }
}

impl<RT: Runtime, T: SystemTable> SystemQuery<'_, '_, RT, T> {
    /// Returns the next document from the query, or `None if the query is
    /// exhausted.
    pub async fn next(&mut self) -> anyhow::Result<Option<Arc<ParsedDocument<T::Metadata>>>>
    where
        T::Metadata: ConvexSerializable,
    {
        if let Some(doc) = self.queue.next() {
            return Ok(Some(doc));
        }
        let (page, _has_more) = self.next_page(*DEFAULT_QUERY_PREFETCH).await?;
        self.queue = page.into_iter();
        Ok(self.queue.next())
    }

    /// Lower-level function for reading one page at a time from the query.
    /// Returns the next (up to) `n` documents and a boolean indicating if more
    /// results are expected. Note that this could return `true` even if there
    /// aren't actually any more documents, if there's still an unexplored index
    /// range that happens to have no documents in it.
    pub async fn next_page(
        &mut self,
        n: usize,
    ) -> anyhow::Result<(Vec<Arc<ParsedDocument<T::Metadata>>>, bool)>
    where
        T::Metadata: ConvexSerializable,
    {
        if self.index_range.is_empty() {
            return Ok((vec![], false));
        }

        if !self.queue.as_slice().is_empty() {
            return Ok((mem::take(&mut self.queue).collect(), true));
        }

        let Ok(tablet_index_id) = self
            .index
            .name
            .clone()
            .map_table(&|_| Ok::<_, !>(self.tablet_id));

        let request = RangeRequest {
            index_name: tablet_index_id,
            printable_index_name: self.index.name(),
            order: self.order,
            interval: mem::replace(&mut self.index_range, Interval::empty()),
            max_size: n,
        };
        let [result] = self
            .tx
            .index
            .range_no_deps(&[&request])
            .await
            .try_into()
            .map_err(|_| anyhow::anyhow!("returned wrong batch size"))?;
        let (page, cursor) = result?;
        let (page_range, remaining) = request.interval.split(cursor, self.order);
        self.index_range = remaining;
        self.tx.reads.record_indexed_directly(
            request.index_name,
            self.index.fields.clone(),
            page_range,
            &self.tx.limits,
        )?;
        if !page.is_empty() {
            let component_path = self
                .tx
                .component_path_for_tablet_id(self.tablet_id)?
                .unwrap_or_default();
            for (_, doc, _) in &page {
                self.tx.reads.record_read_document(
                    component_path.clone(),
                    T::TABLE_NAME.clone(),
                    doc.size(),
                    &self.tx.usage_tracker,
                    &self.tx.virtual_system_mapping,
                    &self.tx.limits,
                )?;
            }
        }

        Ok((
            page.into_iter()
                .map(|(_index_key, doc, _ts)| {
                    Ok(match doc {
                        LazyDocument::Memory(doc) if !T::FOR_MIGRATION => {
                            doc.force::<T::Metadata>()?
                        },
                        LazyDocument::Memory(doc) => Arc::new(doc.parse_uncached()?),
                        LazyDocument::Packed(doc) => Arc::new(doc.parse()?),
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
            !self.index_range.is_empty(),
        ))
    }

    pub fn tx(&mut self) -> &mut Transaction<RT> {
        self.tx
    }
}

impl<RT: Runtime> Transaction<RT> {
    /// Queries the document with the given `id` from table `T`.
    ///
    /// Note that if `id` actually belongs to a different table, this will
    /// return `Ok(None)` (and not raise any error).
    pub async fn get_system<T: SystemTable>(
        &mut self,
        namespace: TableNamespace,
        id: DeveloperDocumentId,
    ) -> anyhow::Result<Option<Arc<ParsedDocument<T::Metadata>>>>
    where
        T::Metadata: ConvexSerializable,
    {
        let index = SystemIndex::<T>::by_id();
        let q = self
            .query_system(namespace, &index)?
            .eq(&[id.encode_into(&mut Default::default())])?;
        let SystemQueryBuilder {
            tx,
            namespace,
            index,
            tablet_id,
            index_range,
            order,
        } = q;
        SystemQueryBuilder {
            tx,
            namespace,
            index,
            tablet_id,
            index_range: Interval::singleton(index_range.prefix.into()),
            order,
        }
        .unique()
        .await
    }
}

impl EqFields {
    fn push<F: ConvexValueWalker + Copy>(
        &mut self,
        fields: &[F],
        indexed_fields_len: usize,
    ) -> anyhow::Result<()> {
        for &value in fields {
            write_sort_key(value, &mut self.prefix).map_err(Into::into)?;
            self.fields += 1;
        }
        // Sanity checks against developer errors
        anyhow::ensure!(
            self.fields <= indexed_fields_len,
            "invalid system query: provided {} eq() fields but index only has {} fields",
            self.fields,
            indexed_fields_len
        );
        Ok(())
    }

    fn range<F: ConvexValueWalker + Copy>(
        self,
        lower_bound: Bound<&[F]>,
        upper_bound: Bound<&[F]>,
        indexed_fields_len: usize,
    ) -> anyhow::Result<Interval> {
        for bound in [lower_bound, lower_bound] {
            if let Bound::Excluded(b) | Bound::Included(b) = bound {
                anyhow::ensure!(
                    self.fields + b.len() <= indexed_fields_len,
                    "invalid system query: provided {} eq() fields and {} bound fields but index \
                     only has {} fields",
                    self.fields,
                    b.len(),
                    indexed_fields_len
                );
            }
        }

        let start = match lower_bound {
            bound @ (Bound::Included(fields) | Bound::Excluded(fields)) => {
                let mut start = self.prefix.clone();
                for &value in fields {
                    write_sort_key(value, &mut start).map_err(Into::into)?;
                }
                if let Bound::Excluded(_) = bound {
                    let Some(key) = BinaryKey::from(start).increment() else {
                        return Ok(Interval::empty());
                    };
                    key
                } else {
                    BinaryKey::from(start)
                }
            },
            Bound::Unbounded => BinaryKey::from(self.prefix.clone()),
        };

        let end = match upper_bound {
            bound @ (Bound::Included(fields) | Bound::Excluded(fields)) => {
                let mut end = self.prefix;
                for &value in fields {
                    write_sort_key(value, &mut end).map_err(Into::into)?;
                }
                if let Bound::Included(_) = bound {
                    End::after_prefix(&end.into())
                } else {
                    End::Excluded(end.into())
                }
            },
            Bound::Unbounded => End::after_prefix(&self.prefix.into()),
        };

        Ok(Interval {
            start: StartIncluded(start),
            end,
        })
    }
}

impl From<EqFields> for Interval {
    #[inline]
    fn from(value: EqFields) -> Self {
        Interval::prefix(value.prefix.into())
    }
}

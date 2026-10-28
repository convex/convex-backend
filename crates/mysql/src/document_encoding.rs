use common::{
    document::ResolvedDocument,
    knobs::MYSQL_DOCUMENT_ENCODING,
};
use value::TabletId;

/// v0 encoding: internal JSON
mod v0 {
    use common::document::ResolvedDocument;
    use value::TabletId;
    pub(crate) fn decode(
        bytes: &[u8],
        table: TabletId,
    ) -> anyhow::Result<Option<ResolvedDocument>> {
        if bytes == b"null" {
            return Ok(None);
        }
        let value = value::json_deserialize_bytes(bytes)?;
        Ok(Some(ResolvedDocument::from_database(table, value)?))
    }
    pub(crate) fn encode(maybe_doc: Option<&ResolvedDocument>) -> anyhow::Result<Vec<u8>> {
        // TODO: we should not bother encoding None documents
        let json_str = match maybe_doc {
            Some(document) => document.value().json_serialize()?,
            None => serde_json::Value::Null.to_string(),
        };
        Ok(json_str.into_bytes())
    }
}

/// v1 encoding: ConvexValue sort key -> block LZ4 with custom dictionary ->
/// prepend version tag & size
mod v1 {
    use bytes::Buf;
    use common::document::ResolvedDocument;
    use value::{
        sorting::write_sort_key,
        walk::ConvexValueType,
        ConvexValue,
        Size,
        TabletId,
    };
    pub(crate) const VERSION_TAG: u8 = 1;

    // Tiny custom dictionary for LZ4 compression. This is required for decoding
    // - don't change this! Every document has a _creationTime and _id field,
    // and we throw in some JSON at the front because it doesn't cost anything
    // and some system documents have internal JSON embedded in them
    const DICT: &[u8] = b"{\"$integer\":\"AAAAAAAAAAA=\"},\x00_id\x00\x10\x15_creationTime\x00\x0d";

    pub(crate) fn decode(
        mut bytes: &[u8],
        table: TabletId,
    ) -> anyhow::Result<Option<ResolvedDocument>> {
        anyhow::ensure!(
            bytes.try_get_u8()? == VERSION_TAG,
            "not a v1 encoded document"
        );
        let uncompressed_size = bytes.try_get_u32()?;
        let decompressed =
            lz4_flex::block::decompress_with_dict(bytes, uncompressed_size.try_into()?, DICT)?;
        let mut reader = &decompressed[..];
        let value = ConvexValue::read_sort_key(&mut reader)?;
        anyhow::ensure!(reader.is_empty(), "trailing bytes");
        Ok(Some(ResolvedDocument::from_database(table, value)?))
    }
    pub(crate) fn encode(maybe_doc: Option<&ResolvedDocument>) -> anyhow::Result<Vec<u8>> {
        // TODO: we should not bother encoding None documents
        let value = match maybe_doc {
            Some(document) => document.value(),
            None => return Ok(vec![]),
        };
        let mut sort_key = Vec::with_capacity(value.size());
        let Ok(()) = write_sort_key(
            ConvexValueType::<&ConvexValue>::Object(value),
            &mut sort_key,
        );
        let mut compressed =
            vec![0u8; 5 + lz4_flex::block::get_maximum_output_size(sort_key.len())];
        compressed[0] = VERSION_TAG;
        compressed[1..5].copy_from_slice(&u32::try_from(sort_key.len())?.to_be_bytes());
        let compressed_len =
            lz4_flex::block::compress_into_with_dict(&sort_key, &mut compressed[5..], DICT)?;
        compressed.truncate(5 + compressed_len);
        Ok(compressed)
    }

}

pub(crate) fn encode(maybe_doc: Option<&ResolvedDocument>) -> anyhow::Result<Vec<u8>> {
    match *MYSQL_DOCUMENT_ENCODING {
        0 => v0::encode(maybe_doc),
        1 => v1::encode(maybe_doc),
        x => anyhow::bail!("Unknown encoding version {x}"),
    }
}
pub(crate) fn decode(bytes: &[u8], table: TabletId) -> anyhow::Result<Option<ResolvedDocument>> {
    match bytes.first() {
        None => Ok(None),
        Some(&b'{' | &b'n') => v0::decode(bytes, table),
        Some(&v1::VERSION_TAG) => v1::decode(bytes, table),
        Some(x) => anyhow::bail!("Unknown encoding version {x}"),
    }
}

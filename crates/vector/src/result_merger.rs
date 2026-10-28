use std::{
    cmp::Reverse,
    collections::BinaryHeap,
};

use futures::TryStreamExt;

use crate::VectorSearchQueryResult;

fn push_results_to_heap(
    heap: &mut BinaryHeap<Reverse<VectorSearchQueryResult>>,
    results: Vec<VectorSearchQueryResult>,
    capacity: usize,
) {
    for result in results {
        // Store Reverse(result) in the heap so that the heap becomes a min-heap
        // instead of the default max-heap. This way, we can evict the smallest
        // element in the heap efficiently once we've reached capacity,
        // leaving us with the top K results.
        heap.push(Reverse(result));
        if heap.len() > capacity {
            heap.pop();
        }
    }
}

fn heap_to_sorted_results(
    heap: BinaryHeap<Reverse<VectorSearchQueryResult>>,
) -> Vec<VectorSearchQueryResult> {
    // BinaryHeap::into_sorted_vec returns results in ascending order of score,
    // but this is a Vec<Reverse<_>>, so the order is already descending, as
    // desired.
    heap.into_sorted_vec().into_iter().map(|r| r.0).collect()
}

/// Merges multiple vectors of VectorSearchQueryResult into top-K results
/// using a min-heap approach.
///
/// The min-heap keeps track of the K best results seen so far. When a new
/// result comes in, it's added to the heap. If the heap exceeds capacity,
/// the smallest element (lowest score) is evicted.
///
/// Results are returned in descending order by score.
pub fn merge_vector_results(
    results_iter: impl Iterator<Item = Vec<VectorSearchQueryResult>>,
    capacity: usize,
) -> Vec<VectorSearchQueryResult> {
    let mut heap: BinaryHeap<Reverse<VectorSearchQueryResult>> =
        BinaryHeap::with_capacity(capacity + 1);

    for results in results_iter {
        push_results_to_heap(&mut heap, results, capacity);
    }

    heap_to_sorted_results(heap)
}

/// Async streaming version of merge_vector_results for use with
/// futures::Stream.
///
/// This is used server-side where segments are fetched and queried as a stream.
/// It uses try_fold to accumulate results into the min-heap as they arrive.
///
/// Results are returned in descending order by score.
pub async fn merge_vector_results_stream<S, E>(
    stream: S,
    capacity: usize,
) -> Result<Vec<VectorSearchQueryResult>, E>
where
    S: futures::Stream<Item = Result<Vec<VectorSearchQueryResult>, E>>,
{
    let heap = stream
        .try_fold(
            BinaryHeap::with_capacity(capacity + 1),
            |mut acc, results| async move {
                push_results_to_heap(&mut acc, results, capacity);
                Ok(acc)
            },
        )
        .await?;

    Ok(heap_to_sorted_results(heap))
}

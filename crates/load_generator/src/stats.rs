use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    time::Duration,
};

use performance_stats::performance::print_histogram;

use crate::event_receiver::{
    Event,
    EventMetadata,
    Metric,
    MetricType,
};

#[derive(Clone)]
pub struct Stats {
    duration: Duration,
    inner: Rc<RefCell<StatsInner>>,
}

struct StatsInner {
    counts: BTreeMap<EventMetadata, u64>,
    latencies: BTreeMap<EventMetadata, Vec<f64>>,
    errors: BTreeMap<EventMetadata, u64>,
}

impl Stats {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            inner: Rc::new(RefCell::new(StatsInner {
                counts: BTreeMap::new(),
                latencies: BTreeMap::new(),
                errors: BTreeMap::new(),
            })),
        }
    }

    fn record_latency(&mut self, value: f64, event_metadata: EventMetadata) {
        let mut inner = self.inner.borrow_mut();
        match inner.latencies.get_mut(&event_metadata) {
            None => {
                inner.latencies.insert(event_metadata, vec![value]);
            },
            Some(q) => {
                q.push(value);
            },
        }
    }

    fn record_count(&mut self, value: u64, event_metadata: EventMetadata) {
        let mut inner = self.inner.borrow_mut();
        let new_count = inner
            .counts
            .get(&event_metadata)
            .map(|count| count + value)
            .unwrap_or_else(|| value);
        inner.counts.insert(event_metadata, new_count);
    }

    fn record_error(&mut self, event_metadata: EventMetadata) {
        let mut inner = self.inner.borrow_mut();
        let new_count = inner
            .errors
            .get(&event_metadata)
            .map(|count| count + 1)
            .unwrap_or_else(|| 1);
        inner.errors.insert(event_metadata, new_count);
    }

    pub fn process_event(&mut self, event: Event) {
        let Event { metadata, metric } = event;

        match metric {
            Metric::Error { msg } => {
                tracing::error!(
                    "ScenarioRunner reported error: {msg} with name {} from scenario {}",
                    metadata.name,
                    metadata.scenario,
                );
                self.record_error(metadata)
            },
            Metric::Metric {
                r#type: MetricType::Latency,
                value,
            } => self.record_latency(value, metadata),
            Metric::Metric {
                r#type: MetricType::Count,
                value,
            } => self.record_count(value as u64, metadata),
        }
    }

    fn histogram(&self, arr: &Vec<f64>) {
        let timings: Vec<Duration> = arr.iter().map(|x| Duration::from_secs_f64(*x)).collect();
        print_histogram(timings);
    }

    /// Some errors are expected, but if there are too many errors recorded in
    /// [`Stats`], fail.
    pub fn fail_if_too_many_errors(&self) -> anyhow::Result<()> {
        let num_errors = self.inner.borrow().errors.values().sum::<u64>();
        let duration_in_secs = self.duration.as_secs();
        // If there are more than an error per second, fail.
        anyhow::ensure!(
            num_errors < duration_in_secs,
            "Too many errors recorded in stats: {num_errors} errors over {duration_in_secs} \
             seconds",
        );
        Ok(())
    }

    pub fn report(&self) {
        let inner = self.inner.borrow();

        println!("Latencies");
        println!("======");
        for (
            EventMetadata {
                name,
                scenario,
                path,
            },
            q,
        ) in inner.latencies.iter()
        {
            println!("Stats for {name} in scenario {scenario} with path {path:?}:");
            println!("QPS: {:.2}", (q.len() as f64) / self.duration.as_secs_f64());
            self.histogram(q);
        }
        println!();
        println!();

        println!("Counts");
        println!("======");
        for (
            EventMetadata {
                name,
                scenario,
                path,
            },
            count,
        ) in inner.counts.iter()
        {
            println!("Rate for {name} in scenario {scenario} with path {path:?}: {count:?}");
        }
        println!();
        println!();

        println!("Errors");
        println!("======");
        for (
            EventMetadata {
                name,
                scenario,
                path,
            },
            count,
        ) in inner.errors.iter()
        {
            println!("Errors for {name} in scenario {scenario} with path {path:?}: {count:?}");
        }
        println!();
        println!();

        println!("Stats report complete!");
    }
}

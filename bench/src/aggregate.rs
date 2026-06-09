use std::{
    collections::{
        HashMap,
        hash_map::Entry::{Occupied, Vacant},
    },
    fmt::Display,
    fs::{File, read_dir},
    io::{BufReader, ErrorKind::UnexpectedEof, Read},
};

use anyhow::Context;

use crate::aggregate::ReconstructedEvent::{PartialReceiverEvent, PartialSenderEvent};

pub struct Aggregation {
    pub aggregation_period_s: f64,
    pub n_windows: usize,
    pub send_delay: Vec<Metric>,
    pub recv_delay: Vec<Metric>,

    pub data_latency: Vec<Metric>,

    pub throughput: Vec<Metric>,
}

struct LazyWindowedMetric {
    n_buckets: usize,
    period: f64,
    start: f64,
    end: f64,
    buckets: Vec<LazyWindowedMetricBucket>,
}

#[derive(Clone)]
struct LazyWindowedMetricBucket {
    start: f64,
    end: f64,
    sorted_values: Vec<f64>,
}

impl LazyWindowedMetric {
    pub fn new(period: f64, start: f64, end: f64) -> Self {
        let n = ((end - start) / period).ceil() as usize;
        Self {
            period,
            n_buckets: n,
            start,
            end,
            buckets: vec![
                LazyWindowedMetricBucket {
                    start: start,
                    end: end,
                    sorted_values: vec![]
                };
                n
            ],
        }
    }

    pub fn add(&mut self, value: f64, time: f64) -> anyhow::Result<()> {
        let bucket = ((time - self.start) / self.period).floor() as usize;

        let dst_i = match self
            .buckets
            .get(bucket)
            .ok_or(anyhow::Error::msg(format!(
                "index {bucket} out of range. bucket count: {}",
                self.n_buckets
            )))?
            .sorted_values
            .binary_search_by(|a| a.total_cmp(&value))
        {
            Ok(i) => i,
            Err(i) => i,
        };
        self.buckets[bucket].sorted_values.insert(dst_i, value);
        Ok(())
    }

    pub fn generate(&self) -> Vec<Metric> {
        let mut result = vec![];
        for bucket in &self.buckets {
            let mut min = f64::MAX;
            let mut max = f64::MIN;

            let res = std_dev::standard_deviation(&bucket.sorted_values);

            let mean = res.mean;
            let std_dev = res.standard_deviation;

            for v in bucket.sorted_values.iter() {
                min = min.min(*v);
                max = max.max(*v);
            }

            result.push(Metric {
                min,
                max,
                mean,
                std_dev,
                p50: percentile(&bucket.sorted_values, 0.5),
                p90: percentile(&bucket.sorted_values, 0.9),
                p95: percentile(&bucket.sorted_values, 0.95),
                p99: percentile(&bucket.sorted_values, 0.99),
                p999: percentile(&bucket.sorted_values, 0.999),
            });
        }
        result
    }
}

fn percentile(sorted_values: &Vec<f64>, p: f64) -> f64 {
    assert!(
        p >= 0.0 && p <= 1.0,
        "percentile only accepts p in range [0.0, 1.0]"
    );

    let n = sorted_values.len();

    let target = p * n as f64;
    let low = target.floor();
    let high = target.ceil();
    let weight = target - low;

    sorted_values[low as usize] * weight + sorted_values[high as usize] * (1.0 - weight)
}

pub enum ReconstructedEvent {
    PartialSenderEvent {
        id: u64,
        start_tx_s: f64,
        end_tx_s: f64,
    },
    PartialReceiverEvent {
        id: u64,
        start_rx_s: f64,
        end_rx_s: f64,
    },
    ReconstructedEvent {
        id: u64,
        start_tx_s: f64,
        end_tx_s: f64,
        start_rx_s: f64,
        end_rx_s: f64,
    },
}

impl Aggregation {
    /// expects directory to include tx_runner_n and rx_runner_n files
    pub fn from_directory(run_path: &'static str) -> anyhow::Result<Aggregation> {
        let aggregation_period_ms = 5_000.0;

        let mut constructed_events: HashMap<u64, ReconstructedEvent> = HashMap::new();

        let mut runner_starts = vec![];
        let mut runner_ends = vec![];

        for entry in read_dir(run_path).context("failed to create path iterator")? {
            let entry = entry.context("failed to read dir entry")?;
            let name = entry.file_name();
            let is_tx = name
                .to_str()
                .ok_or(anyhow::Error::msg(format!(
                    "could not convert {:?} to a string",
                    name
                )))?
                .starts_with("tx");

            let mut file = BufReader::new(File::open(entry.path())?);

            let (runner_start, runner_end, _) = next_binary_row(&mut file)?;

            runner_starts.push(runner_start);
            runner_ends.push(runner_end);

            loop {
                match next_binary_row(&mut file) {
                    Ok((event_start, event_end, id)) => match constructed_events.entry(id) {
                        Occupied(mut e) => {
                            let v = e.get_mut();
                            match v {
                                PartialReceiverEvent {
                                    id,
                                    start_rx_s,
                                    end_rx_s,
                                } => {
                                    if is_tx {
                                        *v = ReconstructedEvent::ReconstructedEvent {
                                            id: *id,
                                            start_tx_s: event_start,
                                            end_tx_s: event_end,
                                            start_rx_s: *end_rx_s,
                                            end_rx_s: *start_rx_s,
                                        }
                                    } else {
                                        return Err(anyhow::Error::msg(format!(
                                            "id {id} already has a PartialReceiverEvent"
                                        )));
                                    }
                                }
                                PartialSenderEvent {
                                    id,
                                    start_tx_s,
                                    end_tx_s,
                                } => {
                                    if !is_tx {
                                        *v = ReconstructedEvent::ReconstructedEvent {
                                            id: *id,
                                            start_tx_s: *start_tx_s,
                                            end_tx_s: *end_tx_s,
                                            start_rx_s: event_start,
                                            end_rx_s: event_end,
                                        }
                                    } else {
                                        return Err(anyhow::Error::msg(format!(
                                            "id {id} already has a PartialSenderEvent"
                                        )));
                                    }
                                }
                                ReconstructedEvent::ReconstructedEvent { .. } => {
                                    unreachable!()
                                }
                            };
                        }
                        Vacant(e) => {
                            if is_tx {
                                e.insert_entry(PartialSenderEvent {
                                    start_tx_s: event_start,
                                    end_tx_s: event_end,
                                    id: id,
                                });
                            } else {
                                e.insert_entry(PartialReceiverEvent {
                                    start_rx_s: event_start,
                                    end_rx_s: event_end,
                                    id: id,
                                });
                            }
                        }
                    },

                    Err(err) => match err.kind() {
                        UnexpectedEof => {
                            break;
                        }
                        _ => return Err(anyhow::Error::from(err).context("error reading row")),
                    },
                }
            }
        }

        let start = *runner_starts
            .iter()
            .min_by(|a, b| a.total_cmp(*b))
            .expect("no min found");
        let end = *runner_ends
            .iter()
            .max_by(|a, b| a.total_cmp(*b))
            .expect("no max found");

        let mut lazy_send_delay = LazyWindowedMetric::new(aggregation_period_ms, start, end);
        let mut lazy_recv_delay = LazyWindowedMetric::new(aggregation_period_ms, start, end);
        let mut lazy_latency = LazyWindowedMetric::new(aggregation_period_ms, start, end);

        let mut lazy_throughput = LazyWindowedMetric::new(aggregation_period_ms, start, end);

        for (id, event) in constructed_events {
            match event {
                PartialSenderEvent {
                    id,
                    start_tx_s,
                    end_tx_s,
                } => {
                    lazy_send_delay.add(end_tx_s - start_tx_s, start_tx_s)?;
                }
                PartialReceiverEvent {
                    id,
                    start_rx_s,
                    end_rx_s,
                } => {
                    lazy_recv_delay.add(end_rx_s - start_rx_s, start_rx_s)?;
                }
                ReconstructedEvent::ReconstructedEvent {
                    id,
                    start_tx_s,
                    end_tx_s,
                    start_rx_s,
                    end_rx_s,
                } => {
                    lazy_send_delay.add(end_tx_s - start_tx_s, start_tx_s)?;
                    lazy_recv_delay.add(end_rx_s - start_rx_s, start_rx_s)?;

                    lazy_latency.add(end_rx_s - start_tx_s, start_tx_s)?;
                    lazy_throughput.add(1.0, end_rx_s)?;
                }
            }
        }

        Ok(Aggregation {
            aggregation_period_s: aggregation_period_ms / 1000.0,
            n_windows: lazy_send_delay.n_buckets,
            send_delay: lazy_send_delay.generate(),
            recv_delay: lazy_recv_delay.generate(),
            data_latency: lazy_latency.generate(),
            throughput: lazy_throughput.generate(),
        })
    }
}
impl Display for Aggregation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut formatted = String::from("Aggregation\n\n");

        formatted = format!(
            "{formatted}Aggregation Period: {}\n",
            self.aggregation_period_s
        );
        for window in 0..self.n_windows {
            formatted = format!("{formatted}Window {}:\n\n\n", window);

            formatted = format!("{formatted}Avg Send Delay:\n{}\n", self.send_delay[window]);
            formatted = format!("{formatted}Receive Delay:\n{}\n", self.recv_delay[window]);
            formatted = format!("{formatted}Latency:\n{}\n", self.data_latency[window]);
            formatted = format!("{formatted}Throughput:\n{}\n", self.throughput[window]);
        }

        write!(f, "{}", formatted)
    }
}

impl Display for Metric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut formatted = format!(
            "Mean:\n\t{:.8}\nStandard Deviation:\n\t{:.8}\nRange\n\t{:.8} --> {:.8}\n",
            self.mean, self.std_dev, self.min, self.max
        );

        formatted = format!("{formatted}Percentiles:\n");
        formatted = format!("{formatted}\tp50, {:.8}\n", self.p50);
        formatted = format!("{formatted}\tp90, {:.8}\n", self.p90);
        formatted = format!("{formatted}\tp95, {:.8}\n", self.p95);
        formatted = format!("{formatted}\tp99, {:.8}\n", self.p99);
        formatted = format!("{formatted}\tp999, {:.8}\n", self.p999);

        write!(f, "{}", formatted)
    }
}

fn next_binary_row(mut r: impl Read) -> std::io::Result<(f64, f64, u64)> {
    let mut header_buf = [0u8; 24];
    r.read_exact(&mut header_buf)?;
    let mut chunks = header_buf.chunks_exact(8);

    Ok((
        f64::from_le_bytes(
            chunks
                .next()
                .expect("chunks did not yield first element")
                .try_into()
                .expect("conversion failed"),
        ),
        f64::from_le_bytes(
            chunks
                .next()
                .expect("chunks did not yield second element")
                .try_into()
                .expect("conversion failed"),
        ),
        u64::from_le_bytes(
            chunks
                .next()
                .expect("chunks did not yield third element")
                .try_into()
                .expect("conversion failed"),
        ),
    ))
}

pub struct Metric {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub std_dev: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub p999: f64,
}

pub mod metric;

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

use crate::aggregate::{
    ReconstructedEvent::{PartialReceiverEvent, PartialSenderEvent},
    metric::{LazyWindowedMetric, Metric},
};

pub struct Aggregation {
    pub aggregation_period_s: f64,
    pub n_windows: usize,
    pub send_delay: Vec<Metric>,
    pub recv_delay: Vec<Metric>,

    pub data_latency: Vec<Metric>,

    pub throughput: Vec<Metric>,
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
        let aggregation_period_ms = 2500.0;

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

        let start_ms = *runner_starts
            .iter()
            .min_by(|a, b| a.total_cmp(*b))
            .expect("no min found")
            * 1000.0;
        let end_ms = *runner_ends
            .iter()
            .max_by(|a, b| a.total_cmp(*b))
            .expect("no max found")
            * 1000.0;

        let mut lazy_send_delay = LazyWindowedMetric::new(aggregation_period_ms, start_ms, end_ms);
        let mut lazy_recv_delay = LazyWindowedMetric::new(aggregation_period_ms, start_ms, end_ms);
        let mut lazy_latency = LazyWindowedMetric::new(aggregation_period_ms, start_ms, end_ms);

        let mut lazy_throughput = LazyWindowedMetric::new(aggregation_period_ms, start_ms, end_ms);

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
            n_windows: lazy_send_delay.n_buckets(),
            send_delay: lazy_send_delay
                .generate()
                .context("failed to generate aggregation for send delay metric")?,
            recv_delay: lazy_recv_delay
                .generate()
                .context("failed to generate aggregation for recv delay metric")?,
            data_latency: lazy_latency
                .generate()
                .context("failed to generate aggregation for latency metric")?,
            throughput: lazy_throughput
                .generate()
                .context("failed to generate aggregation for throughput metric")?,
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

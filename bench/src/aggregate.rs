pub mod metric;

use std::{
    fmt::Display,
    fs::{File, read_dir},
    io::{BufReader, ErrorKind::UnexpectedEof, Read},
    path::Path,
};

use anyhow::Context;
use log::warn;
use serde::{Deserialize, Serialize};

use crate::aggregate::{
    ReconstructedEvent::{PartialReceiverEvent, PartialSenderEvent},
    metric::{DistributionMetric, GaugeMetric, LazyWindowedMetric},
};

const BENCH_DATA_BIN_ROW_LENGTH: usize = 32;

#[derive(Serialize, Deserialize)]
pub struct Aggregation {
    pub start: f64,
    pub end: f64,

    pub sorted_backpressure_values: Vec<(f64, u64)>,

    pub aggregation_period_s: f64,
    pub n_windows: usize,
    pub send_delay: Vec<DistributionMetric>,
    pub recv_delay: Vec<DistributionMetric>,

    pub data_latency: Vec<DistributionMetric>,

    pub throughput: Vec<GaugeMetric>,
}

#[derive(Clone)]
pub enum ReconstructedEvent {
    Empty,
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
    /// expects directory to include tx_runner_n and rx_runner_n files only
    pub fn from_directory(
        run_path: impl AsRef<Path>,
        aggregation_period_s: f64,
    ) -> anyhow::Result<Aggregation> {
        let mut estimated_len: usize = 0;

        for entry in read_dir(&run_path).context("failed to create path iterator")? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                estimated_len +=
                    File::open(path)?.metadata()?.len() as usize / BENCH_DATA_BIN_ROW_LENGTH - 1; // subtract header row
            }
        }
        // event numbers start from 0 and go up by 1, we can just use a cheap vec
        let mut constructed_events: Vec<ReconstructedEvent> = Vec::with_capacity(estimated_len);

        let mut start = f64::MAX;
        let mut end = f64::MIN;

        let mut backpressure: Vec<(f64, u64)> = vec![];

        for entry in read_dir(&run_path).context("failed to create path iterator")? {
            let entry = entry.context("failed to read dir entry")?;
            let name = entry.file_name();
            let is_tx = {
                let name = name.to_str().ok_or(anyhow::Error::msg(format!(
                    "could not convert {:?} to a string",
                    name
                )))?;
                if name.starts_with("tx") {
                    true
                } else if name.starts_with("rx") {
                    false
                } else {
                    warn!("unrecognized file {}, skipping", name);
                    continue;
                }
            };
            let file = File::open(entry.path())?;

            let mut file = BufReader::new(file);

            let (runner_start, runner_end, _, _) = next_binary_row(&mut file)?;

            start = start.min(runner_start);
            end = end.max(runner_end);

            loop {
                match next_binary_row(&mut file) {
                    Ok((event_start, event_end, event_id, event_backpressure)) => {
                        backpressure.push((event_end, event_backpressure));
                        if constructed_events.len() < event_id as usize + 1 {
                            constructed_events
                                .resize(event_id as usize + 1, ReconstructedEvent::Empty);
                        }
                        let entry = &mut constructed_events[event_id as usize];
                        match entry {
                            PartialReceiverEvent {
                                id,
                                start_rx_s,
                                end_rx_s,
                            } => {
                                if is_tx {
                                    *entry = ReconstructedEvent::ReconstructedEvent {
                                        id: *id,
                                        start_tx_s: event_start,
                                        end_tx_s: event_end,
                                        start_rx_s: *start_rx_s,
                                        end_rx_s: *end_rx_s,
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
                                    *entry = ReconstructedEvent::ReconstructedEvent {
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

                            ReconstructedEvent::Empty => {
                                if is_tx {
                                    *entry = PartialSenderEvent {
                                        start_tx_s: event_start,
                                        end_tx_s: event_end,
                                        id: event_id,
                                    };
                                } else {
                                    *entry = PartialReceiverEvent {
                                        start_rx_s: event_start,
                                        end_rx_s: event_end,
                                        id: event_id,
                                    };
                                }
                            }
                        }
                    }

                    Err(err) => match err.kind() {
                        UnexpectedEof => {
                            break;
                        }
                        _ => return Err(anyhow::Error::from(err).context("error reading row")),
                    },
                }
            }
        }

        let mut lazy_send_delay = LazyWindowedMetric::new(aggregation_period_s, start, end);
        let mut lazy_recv_delay = LazyWindowedMetric::new(aggregation_period_s, start, end);
        let mut lazy_latency = LazyWindowedMetric::new(aggregation_period_s, start, end);

        let mut lazy_throughput = LazyWindowedMetric::new(aggregation_period_s, start, end);

        let mut empty = 0usize;
        for event in constructed_events {
            match event {
                PartialSenderEvent {
                    id: _,
                    start_tx_s,
                    end_tx_s,
                } => {
                    lazy_send_delay.add(end_tx_s - start_tx_s, start_tx_s)?;
                }
                PartialReceiverEvent {
                    id: _,
                    start_rx_s,
                    end_rx_s,
                } => {
                    lazy_recv_delay.add(end_rx_s - start_rx_s, start_rx_s)?;
                }
                ReconstructedEvent::ReconstructedEvent {
                    id: _,
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
                ReconstructedEvent::Empty => empty += 1,
            }
        }

        if empty > 0 {
            warn!("found {} skipped event ids", empty);
        }
        backpressure.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));

        Ok(Aggregation {
            start: start,
            end: end,

            sorted_backpressure_values: backpressure,

            aggregation_period_s,
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
                .generate_gauged()
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

fn next_binary_row(mut r: impl Read) -> std::io::Result<(f64, f64, u64, u64)> {
    let mut header_buf = [0u8; 32];
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
        u64::from_le_bytes(
            chunks
                .next()
                .expect("chunks did not yield third element")
                .try_into()
                .expect("conversion failed"),
        ),
    ))
}

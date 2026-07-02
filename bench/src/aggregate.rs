pub mod metric;

use std::{
    fs::{File, read_dir},
    io::{BufReader, Read},
    path::Path,
};

use anyhow::Context;
use log::warn;
use memmap2::Mmap;
use serde::{Deserialize, Serialize};

use crate::aggregate::{
    ReconstructedEvent::{PartialReceiverEvent, PartialSenderEvent},
    metric::LazyWindowedMetric,
};

const BENCH_DATA_BIN_ROW_LENGTH: usize = 32;

#[derive(Serialize, Deserialize)]
pub struct Aggregation {
    pub start: f64,
    pub end: f64,
    pub aggregation_period_s: f64,
    pub n_windows: usize,

    pub backpressure: GaugeSummary,
    pub throughput: GaugeSummary,
    pub latency: DistributionSummary,
    pub send: DistributionSummary,
    pub recv: DistributionSummary,
}

#[derive(Serialize, Deserialize)]
pub struct DistributionSummary {
    pub t: Vec<f64>,
    pub mean: f64,
    pub count: usize,
    pub p50: Vec<f64>,
    pub overall_p50: f64,
    pub p99: Vec<f64>,
    pub overall_p99: f64,
    pub p999: Vec<f64>,
    pub overall_p999: f64,
    pub max: f64,
}

#[derive(Serialize, Deserialize)]
pub struct GaugeSummary {
    pub t: Vec<f64>,
    pub values: Vec<f64>,
    pub mean: f64,
    pub max: f64,
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
    ///
    /// # Safety
    ///
    /// Must confirm that no other processes read/write to any file in the provided run_path.
    pub unsafe fn from_directory(
        run_path: impl AsRef<Path>,
        run_bin_path: impl AsRef<Path>,
        aggregation_period_s: f64,
    ) -> anyhow::Result<Aggregation> {
        let mut estimated_len: usize = 0;
        let mut start = [0u8; 8];
        let mut end = [0u8; 8];
        let mut reader = BufReader::new(File::open(run_bin_path)?);
        let read = reader.read(&mut start)?;
        assert_eq!(8, read);
        let read = reader.read(&mut end)?;
        assert_eq!(8, read);

        let start = f64::from_le_bytes(start);
        let end = f64::from_le_bytes(end);

        for entry in read_dir(&run_path).context("failed to create path iterator")? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                estimated_len +=
                    File::open(path)?.metadata()?.len() as usize / BENCH_DATA_BIN_ROW_LENGTH - 1; // subtract header row
            }
        }
        // event numbers start from 0 and go up by 1, we can just use a cheap vec
        let mut constructed_events: Vec<ReconstructedEvent> = Vec::with_capacity(estimated_len / 2);

        let mut bp = LazyWindowedMetric::new(aggregation_period_s, start, end);

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

            let path = entry.path();
            let file = File::open(&path)?;
            file.lock()
                .context(format!("could not lock file at {:?}", path))?;

            // Safety:
            // - A (potentially non-mandatory) lock on the current file is held.
            // - Safety confirmation propogated up to client of this library.
            let m_mapped_file = unsafe { Mmap::map(&file) }
                .context(format!("could not memory-map file at {:?}", path))?;
            let mut rows = m_mapped_file
                .as_chunks::<BENCH_DATA_BIN_ROW_LENGTH>()
                .0
                .iter();
            // discard header row
            let _ = rows.next();

            for row in rows {
                let (event_start, event_end, event_id, event_backpressure) = parse_row(row);
                {
                    bp.add(event_backpressure as f64, event_end)?;
                    if constructed_events.len() < event_id as usize + 1 {
                        constructed_events.resize(event_id as usize + 1, ReconstructedEvent::Empty);
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
            }
            file.unlock()
                .context(format!("could not unlock file at {:?}", path))?;
        }

        let mut lazy_send_delay = LazyWindowedMetric::new(aggregation_period_s, start, end);
        let mut lazy_recv_delay = LazyWindowedMetric::new(aggregation_period_s, start, end);
        let mut lazy_latency = LazyWindowedMetric::new(aggregation_period_s, start, end);
        let mut lazy_throughput = LazyWindowedMetric::new(aggregation_period_s, start, end);
        let n_windows = lazy_send_delay.n_buckets();

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

        let throughput = lazy_throughput
            .generate_gauged(|iter, start, end| iter.sum::<f64>() / (end - start))
            .context("failed to generate aggregation for throughput metric")?;

        let backpressure = bp
            .generate_gauged(|iter, _, _| {
                *iter
                    .min_by(|a, b| a.total_cmp(b))
                    .context("unable to calculate min")
                    .unwrap()
            })
            .context("failed to generate backpressure metrics")?;

        let send = lazy_send_delay
            .generate()
            .context("failed to generate aggregation for send delay metric")?;
        let recv = lazy_recv_delay
            .generate()
            .context("failed to generate aggregation for recv delay metric")?;
        let latency = lazy_latency
            .generate()
            .context("failed to generate aggregation for latency metric")?;

        Ok(Aggregation {
            start,
            end,
            aggregation_period_s,
            n_windows,

            latency,
            send,
            recv,
            backpressure,
            throughput,
        })
    }
}

fn parse_row(row: &[u8; 32]) -> (f64, f64, u64, u64) {
    let row = row.as_chunks::<8>().0;
    (
        f64::from_le_bytes(row[0]),
        f64::from_le_bytes(row[1]),
        u64::from_le_bytes(row[2]),
        u64::from_le_bytes(row[3]),
    )
}

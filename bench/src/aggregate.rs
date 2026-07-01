pub mod metric;

use std::{
    fs::{File, read_dir},
    io::{BufReader, ErrorKind::UnexpectedEof, Read},
    path::Path,
};

use anyhow::Context;
use log::warn;
use serde::{Deserialize, Serialize};

use crate::aggregate::{
    ReconstructedEvent::{PartialReceiverEvent, PartialSenderEvent},
    metric::{DistributionMetric, GaugeMetric, LazyWindowedMetric, percentile},
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
    pub fn from_directory(
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
        let mut constructed_events: Vec<ReconstructedEvent> = Vec::with_capacity(estimated_len);

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
            let file = File::open(entry.path())?;

            let mut file = BufReader::new(file);

            // discard header row
            let _ = next_binary_row(&mut file)?;

            loop {
                match next_binary_row(&mut file) {
                    Ok((event_start, event_end, event_id, event_backpressure)) => {
                        bp.add(event_backpressure as f64, event_end)?;
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

        let tp = lazy_throughput
            .generate_gauged(|iter, start, end| iter.sum::<f64>() / (end - start))
            .context("failed to generate aggregation for throughput metric")?;

        let mut t_tp = Vec::with_capacity(tp.len());
        let mut throughput = Vec::with_capacity(tp.len());
        let mut tp_max = f64::MIN;
        for metric in tp {
            if let GaugeMetric::Gauge { start, value, .. } = metric {
                t_tp.push(start);
                throughput.push(value);
                tp_max = tp_max.max(value);
            }
        }

        let bp = bp
            .generate_gauged(|iter, _, _| {
                *iter
                    .min_by(|a, b| a.total_cmp(b))
                    .context("unable to calculate min")
                    .unwrap()
            })
            .context("failed to generate backpressure metrics")?;

        let mut backpressure = Vec::with_capacity(bp.len());
        let mut t_bp = Vec::with_capacity(bp.len());
        let mut max_bp = f64::MIN;
        for metric in bp {
            if let GaugeMetric::Gauge { value, start, .. } = metric {
                t_bp.push(start);
                backpressure.push(value);
                max_bp = max_bp.max(value);
            }
        }

        let send = lazy_send_delay
            .generate()
            .context("failed to generate aggregation for send delay metric")?;
        let recv = lazy_recv_delay
            .generate()
            .context("failed to generate aggregation for recv delay metric")?;
        let latency = lazy_latency
            .generate()
            .context("failed to generate aggregation for latency metric")?;

        let mut t_send = Vec::with_capacity(send.len());
        let mut send_p50 = Vec::with_capacity(send.len());
        let mut send_p99 = Vec::with_capacity(send.len());
        let mut send_p999 = Vec::with_capacity(send.len());
        let mut send_max = f64::MIN;
        let mut send_mean = 0.0;
        let mut send_count = 0;
        let mut send_values = vec![];
        for metric in send {
            if let DistributionMetric::Distribution {
                start,
                p50,
                p99,
                p999,
                max,
                mean,
                count,
                raw_values,
                ..
            } = metric
            {
                t_send.push(start);
                send_p50.push(p50);
                send_p99.push(p99);
                send_p999.push(p999);
                send_max = send_max.max(max);
                send_mean = (send_mean * send_count as f64 + mean * count as f64)
                    / (send_count + count) as f64;
                send_count += count;
                send_values.extend(raw_values);
            }
        }

        let mut t_recv = Vec::with_capacity(recv.len());
        let mut recv_p50 = Vec::with_capacity(recv.len());
        let mut recv_p99 = Vec::with_capacity(recv.len());
        let mut recv_p999 = Vec::with_capacity(recv.len());
        let mut recv_max = f64::MIN;
        let mut recv_mean = 0.0;
        let mut recv_count = 0;
        let mut recv_values = vec![];
        for metric in recv {
            if let DistributionMetric::Distribution {
                start,
                p50,
                p99,
                p999,
                max,
                mean,
                count,
                raw_values,
                ..
            } = metric
            {
                t_recv.push(start);
                recv_p50.push(p50);
                recv_p99.push(p99);
                recv_p999.push(p999);
                recv_max = recv_max.max(max);
                recv_mean = (recv_mean * (recv_count as f64) + mean * (count as f64))
                    / (recv_count as f64 + count as f64);
                recv_count += count;
                recv_values.extend(raw_values);
            }
        }

        let mut t_lat = Vec::with_capacity(latency.len());
        let mut latency_p50 = Vec::with_capacity(latency.len());
        let mut latency_p99 = Vec::with_capacity(latency.len());
        let mut latency_p999 = Vec::with_capacity(latency.len());
        let mut latency_max = f64::MIN;
        let mut latency_mean = 0.0;
        let mut latency_count = 0;
        let mut latency_values = vec![];
        for metric in latency {
            if let DistributionMetric::Distribution {
                start,
                p50,
                p99,
                p999,
                max,
                mean,
                count,
                raw_values,
                ..
            } = metric
            {
                t_lat.push(start);
                latency_p50.push(p50);
                latency_p99.push(p99);
                latency_p999.push(p999);
                latency_max = latency_max.max(max);
                latency_mean = (latency_mean * (latency_count as f64) + mean * (count as f64))
                    / (latency_count as f64 + count as f64);
                latency_count += count;
                latency_values.extend(raw_values);
            }
        }

        latency_values.sort_unstable_by(f64::total_cmp);
        recv_values.sort_unstable_by(f64::total_cmp);
        send_values.sort_unstable_by(f64::total_cmp);

        Ok(Aggregation {
            start,
            end,
            aggregation_period_s,
            n_windows: lazy_send_delay.n_buckets(),

            latency: DistributionSummary {
                t: t_lat,
                mean: latency_mean,
                count: latency_count,
                overall_p50: percentile(&latency_values, 0.5)?,
                overall_p99: percentile(&latency_values, 0.99)?,
                overall_p999: percentile(&latency_values, 0.999)?,
                p50: latency_p50,
                p99: latency_p99,
                p999: latency_p999,
                max: latency_max,
            },
            send: DistributionSummary {
                t: t_send,
                mean: send_mean,
                count: send_count,
                overall_p50: percentile(&send_values, 0.5)?,
                overall_p99: percentile(&send_values, 0.99)?,
                overall_p999: percentile(&send_values, 0.999)?,
                p50: send_p50,
                p99: send_p99,
                p999: send_p999,
                max: send_max,
            },
            recv: DistributionSummary {
                t: t_recv,
                mean: recv_mean,
                count: recv_count,
                overall_p50: percentile(&recv_values, 0.5)?,
                overall_p99: percentile(&recv_values, 0.99)?,
                overall_p999: percentile(&recv_values, 0.999)?,
                p50: recv_p50,
                p99: recv_p99,
                p999: recv_p999,
                max: recv_max,
            },
            backpressure: GaugeSummary {
                t: t_bp,
                mean: backpressure.iter().sum::<f64>() / backpressure.len() as f64,
                values: backpressure,
                max: max_bp,
            },
            throughput: GaugeSummary {
                t: t_tp,
                mean: throughput.iter().sum::<f64>() / throughput.len() as f64,
                values: throughput,
                max: tp_max,
            },
        })
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

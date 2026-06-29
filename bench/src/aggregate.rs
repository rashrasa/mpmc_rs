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
    metric::{DistributionMetric, GaugeMetric, LazyWindowedMetric},
};

const BENCH_DATA_BIN_ROW_LENGTH: usize = 32;

#[derive(Serialize, Deserialize)]
pub struct Aggregation {
    pub start: f64,
    pub end: f64,
    pub aggregation_period_s: f64,
    pub n_windows: usize,

    pub t_bp: Vec<f64>,
    pub backpressure: Vec<f64>,
    pub max_backpressure: f64,

    pub t_tp: Vec<f64>,
    pub throughput: Vec<f64>,
    pub max_throughput: f64,

    pub t_lat: Vec<f64>,
    pub latency_p50: Vec<f64>,
    pub latency_p99: Vec<f64>,
    pub latency_p999: Vec<f64>,
    pub latency_max: f64,

    pub t_send: Vec<f64>,
    pub send_p50: Vec<f64>,
    pub send_p99: Vec<f64>,
    pub send_p999: Vec<f64>,
    pub send_max: f64,

    pub t_recv: Vec<f64>,
    pub recv_p50: Vec<f64>,
    pub recv_p99: Vec<f64>,
    pub recv_p999: Vec<f64>,
    pub recv_max: f64,
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

        let mut time_bp: Vec<(f64, u64)> = vec![];

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
                        time_bp.push((event_end, event_backpressure));
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
            .generate_gauged()
            .context("failed to generate aggregation for throughput metric")?;

        let max_bp = time_bp.iter().fold(0, |a, b| a.max(b.1));

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

        let mut backpressure = Vec::with_capacity(time_bp.len());
        let mut t_bp = vec![];
        for (t, bp) in time_bp {
            t_bp.push(t);
            backpressure.push(bp as f64);
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
        for metric in send {
            if let DistributionMetric::Distribution {
                start,
                p50,
                p99,
                p999,
                max,
                ..
            } = metric
            {
                t_send.push(start);
                send_p50.push(p50);
                send_p99.push(p99);
                send_p999.push(p999);
                send_max = send_max.max(max);
            }
        }

        let mut t_recv = Vec::with_capacity(recv.len());
        let mut recv_p50 = Vec::with_capacity(recv.len());
        let mut recv_p99 = Vec::with_capacity(recv.len());
        let mut recv_p999 = Vec::with_capacity(recv.len());
        let mut recv_max = f64::MIN;
        for metric in recv {
            if let DistributionMetric::Distribution {
                start,
                p50,
                p99,
                p999,
                max,
                ..
            } = metric
            {
                t_recv.push(start);
                recv_p50.push(p50);
                recv_p99.push(p99);
                recv_p999.push(p999);
                recv_max = recv_max.max(max);
            }
        }

        let mut t_lat = Vec::with_capacity(latency.len());
        let mut latency_p50 = Vec::with_capacity(latency.len());
        let mut latency_p99 = Vec::with_capacity(latency.len());
        let mut latency_p999 = Vec::with_capacity(latency.len());
        let mut latency_max = f64::MIN;
        for metric in latency {
            if let DistributionMetric::Distribution {
                start,
                p50,
                p99,
                p999,
                max,
                ..
            } = metric
            {
                t_lat.push(start);
                latency_p50.push(p50);
                latency_p99.push(p99);
                latency_p999.push(p999);
                latency_max = latency_max.max(max);
            }
        }

        Ok(Aggregation {
            start,
            end,
            aggregation_period_s,
            n_windows: lazy_send_delay.n_buckets(),

            backpressure,
            max_backpressure: max_bp as f64,
            t_bp,
            t_tp,
            throughput,
            max_throughput: tp_max,
            t_lat,
            latency_p50,
            latency_p99,
            latency_p999,
            latency_max,
            t_send,
            send_p50,
            send_p99,
            send_p999,
            send_max,
            t_recv,
            recv_p50,
            recv_p99,
            recv_p999,
            recv_max,
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

fn dedupe_bp_values(backpressure: Vec<(f64, u64)>) -> Vec<(f64, u64)> {
    let bp_n = backpressure.len();
    if bp_n > 0 {
        let mut vec = Vec::with_capacity(bp_n);

        let mut backpressure_iter = backpressure.into_iter();
        let (mut last_t, mut sum_v) = backpressure_iter.next().unwrap();
        let mut last_n = 1;

        for (t, v) in backpressure_iter {
            if last_t != t {
                vec.push((last_t, sum_v / last_n));
                last_t = t;
                sum_v = v;
                last_n = 1;
            } else {
                last_n += 1;
                sum_v += v;
            }
        }

        vec.push((last_t, sum_v / last_n));
        vec
    } else {
        vec![]
    }
}

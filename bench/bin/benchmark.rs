use std::path::Path;

use anyhow::Context;
use fast_time::Clock;

use bench::{
    runner::MainBenchRunner,
    test::test_1::{self, run_bench_1},
};
use log::{error, info};
use mpac_rs::{v1::V1Maker, v2::V2Maker, v3::V3Maker};

enum Version {
    V1(&'static str),
    V2(&'static str),
    V3(&'static str),
}

/// ## Bench:
///
/// - 1 core reserved for gathering metrics
///
/// ### Metrics (Mean, P{50, 90, 99, 999}):
///
/// - Send/Receive Throughput
/// - Sender/Receiver Latency
///
/// #### Other
///
/// - Metrics' scaling with # of Sender/Receiver threads
///
/// ### Scenarios
///
/// - Pure value channel
///     - 1-1, 7-1, 1-7, 4-4, 7-7 sender-receiver threads
///     - T sizes: 4 bytes, 64 bytes, 8 kB, 64 kB
///
/// - One request and one response channel
///     - 1-1, 4-4, 6-1, 1-6 sender-receiver threads for each channel
/// - Sending sequenced data which has to be re-constructed and ordered by receivers
///     - 1 unique series per sender
///     - all receivers need to cooperate for each series and maintain a collection of sequenced values
///     - (1-1, 7-1, 1-7, 4-4, 7-7)
fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Debug)
        .init();

    let mut v1 = false;
    let mut v2 = false;
    let mut v3 = false;

    for arg in std::env::args() {
        if arg == "v1" {
            v1 = true;
        }
        if arg == "v2" {
            v2 = true;
        }
        if arg == "v3" {
            v3 = true;
        }
    }

    if !v1 && !v2 && !v3 {
        v1 = true;
        v2 = true;
        v3 = true;
    }

    let mut version_descs = vec![];
    if v1 {
        version_descs.push(Version::V1("v1_naive"));
    }
    if v2 {
        version_descs.push(Version::V2("v2_vec_deque"));
    }
    if v3 {
        version_descs.push(Version::V3("v3_lock_free"));
    }

    // version names: tx_rx_sttl_rttl_size
    let configs = vec![
        (
            "3_3_5_5_4",
            test_1::Config {
                n_senders: 3,
                n_receivers: 3,
                sender_ttl_s: Some(5.0),
                receiver_ttl_s: Some(5.0),
                make_payload: || 9u32,
            },
        ),
        (
            "1_3_5_5_4",
            test_1::Config {
                n_senders: 1,
                n_receivers: 3,
                sender_ttl_s: Some(5.0),
                receiver_ttl_s: Some(5.0),
                make_payload: || 9u32,
            },
        ),
        (
            "3_1_5_5_4",
            test_1::Config {
                n_senders: 3,
                n_receivers: 1,
                sender_ttl_s: Some(5.0),
                receiver_ttl_s: Some(5.0),
                make_payload: || 9u32,
            },
        ),
        (
            "7_7_10_10_4",
            test_1::Config {
                n_senders: 7,
                n_receivers: 7,
                sender_ttl_s: Some(10.0),
                receiver_ttl_s: Some(10.0),
                make_payload: || 9u32,
            },
        ),
    ];

    info!("Starting benchmark");
    let mut clock = Clock::new();
    let start = clock.now();

    let main_runner = MainBenchRunner::new(Path::new("output/result").to_path_buf());

    for v in &version_descs {
        let version_desc = match v {
            Version::V1(d) => d,
            Version::V2(d) => d,
            Version::V3(d) => d,
        };
        let runner = main_runner.spawn_runner(format!("version_{}", version_desc));
        for (config_desc, config) in &configs {
            main_runner.reset_ids();
            info!(
                "Starting {} tests with profile {}",
                version_desc, config_desc
            );
            let runner = runner.spawn_runner(format!("config_{}", config_desc));

            match v {
                Version::V1(_) => run_bench_1(&runner, V1Maker, config.clone())
                    .context("failed to run benchmark 1")?,
                Version::V2(_) => run_bench_1(&runner, V2Maker, config.clone())
                    .context("failed to run benchmark 1")?,
                Version::V3(_) => run_bench_1(&runner, V3Maker, config.clone())
                    .context("failed to run benchmark 1")?,
            }

            if let Err(err) = runner.complete_runner() {
                error!("{:?}", err);
            }
        }
        if let Err(err) = runner.complete_runner() {
            error!("{:?}", err);
        }
    }

    if let Err(err) = main_runner.complete_runner() {
        error!("{:?}", err);
    }

    info!(
        "Benchmarks completed. Ran for {}",
        start.elapsed(&mut clock).as_secs_f64()
    );

    Ok(())
}

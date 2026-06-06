use std::time::Instant;

use bench::{
    bench::bench_1::{self, run_bench_1},
    runner::MainBenchRunner,
};
use log::info;

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
fn main() {
    env_logger::builder()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Debug)
        .init();
    info!("Starting benchmark");
    let start = Instant::now();
    let makers = vec![(
        "v1_naive",
        Box::new(mpac_rs::v1::V1Maker),
        vec![
            (
                "1_tx-7_rx_ttl_5",
                bench_1::Config {
                    n_senders: 7,
                    n_receivers: 1,
                    sender_config: bench_1::SenderConfig::TimeToLiveSeconds(5.0),
                },
            ),
            (
                "7_tx-1_rx_ttl_5",
                bench_1::Config {
                    n_senders: 1,
                    n_receivers: 7,
                    sender_config: bench_1::SenderConfig::TimeToLiveSeconds(5.0),
                },
            ),
            (
                "1_tx-7_rx_nor_100k",
                bench_1::Config {
                    n_senders: 7,
                    n_receivers: 1,
                    sender_config: bench_1::SenderConfig::NumberOfRequests(100_000),
                },
            ),
            (
                "7_tx-1_rx_nor_100k",
                bench_1::Config {
                    n_senders: 1,
                    n_receivers: 7,
                    sender_config: bench_1::SenderConfig::NumberOfRequests(100_000),
                },
            ),
        ],
    )];
    let runner = MainBenchRunner::new();

    for (version_desc, version, configs) in makers {
        let runner = runner.spawn_runner(format!("version_{}", version_desc));
        for (config_desc, config) in configs {
            info!(
                "Starting {} tests with profile {}",
                version_desc, config_desc
            );
            let runner = runner.spawn_runner(format!("config_{}", config_desc));
            run_bench_1(&runner, version.as_ref(), config);
        }
    }

    info!(
        "Benchmarks completed. Ran for {}",
        start.elapsed().as_secs_f64()
    );
}

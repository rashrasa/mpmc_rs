#![feature(test)]
#![allow(unused_features)]

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
                "3_tx-1_rx_ttl_5",
                bench_1::Config {
                    n_senders: 3,
                    n_receivers: 1,
                    sender_config: bench_1::SenderConfig::TimeToLiveSeconds(1.0),
                },
            ),
            (
                "1_tx-3_rx_ttl_5",
                bench_1::Config {
                    n_senders: 1,
                    n_receivers: 3,
                    sender_config: bench_1::SenderConfig::TimeToLiveSeconds(1.0),
                },
            ),
            (
                "3_tx-1_rx_nor_100k",
                bench_1::Config {
                    n_senders: 3,
                    n_receivers: 1,
                    sender_config: bench_1::SenderConfig::NumberOfRequests(100_000),
                },
            ),
            (
                "1_tx-3_rx_nor_100k",
                bench_1::Config {
                    n_senders: 1,
                    n_receivers: 3,
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

#[cfg(test)]
mod tests {

    extern crate test;
    use fast_time::Clock;
    use std::time::{Instant, SystemTime};
    use test::Bencher;

    #[bench]
    fn bench_instant_now(bencher: &mut Bencher) {
        bencher.iter(|| Instant::now());
    }

    #[bench]
    fn bench_instant_elapsed_f64(bencher: &mut Bencher) {
        let now = Instant::now();

        bencher.iter(|| now.elapsed().as_secs_f64());
    }

    #[bench]
    fn bench_system_time_now(bencher: &mut Bencher) {
        bencher.iter(|| SystemTime::now());
    }

    #[bench]
    fn bench_system_time_elapsed_f64(bencher: &mut Bencher) {
        let now = SystemTime::now();

        bencher.iter(|| now.elapsed().unwrap().as_secs_f64());
    }

    #[bench]
    fn bench_fast_time_now(bencher: &mut Bencher) {
        let mut clock = Clock::new();

        bencher.iter(|| clock.now());
    }

    #[bench]
    fn bench_fast_time_elapsed_f64(bencher: &mut Bencher) {
        let mut clock = Clock::new();
        let now = clock.now();

        bencher.iter(|| now.elapsed(&mut clock).as_secs_f64());
    }
}

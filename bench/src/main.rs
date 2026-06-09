use std::{
    fs::File,
    io::{BufWriter, Write},
};

use anyhow::Context;
use fast_time::Clock;

use bench::{
    aggregate::Aggregation,
    bench::bench_1::{self, run_bench_1},
    runner::MainBenchRunner,
};
use log::{error, info};

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

    // version names: tx_rx_sttl_rttl_size
    let makers = vec![(
        "v1_naive",
        Box::new(mpac_rs::v1::V1Maker),
        vec![(
            "3_3_10_10_4",
            bench_1::Config {
                n_senders: 3,
                n_receivers: 3,
                sender_ttl_s: Some(10.0),
                receiver_ttl_s: Some(10.0),
                make_payload: || 9u32,
            },
        )],
    )];

    let agg =
        Aggregation::from_directory("./results/main_runner/version_v1_naive/config_3_3_10_10_4")
            .context("could not run aggregation")?;

    let mut file = BufWriter::new(File::create("aggregation.txt").context("could not open file")?);

    file.write_all(&format!("{}", agg).into_bytes())?;

    // TODO: Use command-line arguments to choose whether to aggregate or benchmark
    return Ok(());
    info!("Starting benchmark");
    let mut clock = Clock::new();
    let start = clock.now();

    let runner = MainBenchRunner::new();

    for (version_desc, version, configs) in makers {
        let runner = runner.spawn_runner(format!("version_{}", version_desc));
        for (config_desc, config) in configs {
            info!(
                "Starting {} tests with profile {}",
                version_desc, config_desc
            );
            let runner = runner.spawn_runner(format!("config_{}", config_desc));
            run_bench_1(&runner, version.as_ref(), config).context("failed to run benchmark 1")?;

            if let Err(err) = runner.complete_runner() {
                error!("{:?}", err);
            }
        }
        if let Err(err) = runner.complete_runner() {
            error!("{:?}", err);
        }
    }
    if let Err(err) = runner.complete_runner() {
        error!("{:?}", err);
    }

    info!(
        "Benchmarks completed. Ran for {}",
        start.elapsed(&mut clock).as_secs_f64()
    );

    Ok(())
}

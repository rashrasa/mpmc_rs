use std::{collections::HashSet, path::Path, time::Instant};

use anyhow::Context;

use bench::{
    runner::MainBenchRunner,
    test::test_1::{self, run_bench_1},
};
use log::{debug, error, info};
use mpmc_rs::{
    external::CrossbeamMaker, v1::V1Maker, v2::V2Maker, v3::V3Maker, v4::V4Maker, v5::V5Maker,
};

enum Version {
    V1(&'static str),
    V2(&'static str),
    V3(&'static str),
    V4(&'static str),
    V5(&'static str),
    Crossbeam(&'static str),
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
    let mut v4 = false;
    let mut v5 = false;
    let mut crossbeam = false;
    let mut iter = std::env::args();

    // ignore filename
    iter.next();

    for arg in iter {
        match arg.as_str() {
            "v1" => {
                v1 = true;
            }
            "v2" => {
                v2 = true;
            }
            "v3" => {
                v3 = true;
            }
            "v4" => {
                v4 = true;
            }
            "v5" => {
                v5 = true;
            }
            "crossbeam" => {
                crossbeam = true;
            }
            _ => {
                unimplemented!("unknown arg {}", arg);
            }
        }
    }

    if !v1 && !v2 && !v3 && !v4 && !v5 && !crossbeam {
        v1 = true;
        v2 = true;
        v3 = true;
        v4 = true;
        v5 = true;
        crossbeam = true;
    }

    let mut version_descs = vec![];
    if v1 {
        version_descs.push(Version::V1("v1_naive"));
    }
    if v2 {
        version_descs.push(Version::V2("v2_vec_deque"));
    }
    if v3 {
        version_descs.push(Version::V3("v3_locked_ends"));
    }
    if v4 {
        version_descs.push(Version::V4("v4_lock_free_array"));
    }
    if v5 {
        version_descs.push(Version::V5("v5_parking_lot"));
    }
    if crossbeam {
        version_descs.push(Version::Crossbeam("crossbeam_mpmc_unbounded"));
    }

    // version names: tx_rx_sttl_rttl_size
    let mut configs = HashSet::new();

    debug!("creating configs");
    configs.extend(create_configs_ramping((1, 1), (7, 7), (3.0, 3.0), || 9u32));
    configs.extend(create_configs_ramping((1, 2), (1, 7), (3.0, 3.0), || 9u32));
    configs.extend(create_configs_ramping((2, 1), (7, 1), (3.0, 3.0), || 9u32));

    info!("Starting benchmark");
    let start = Instant::now();

    let main_runner = MainBenchRunner::new(Path::new("output/result").to_path_buf());

    for v in &version_descs {
        let version_desc = match v {
            Version::V1(d) => d,
            Version::V2(d) => d,
            Version::V3(d) => d,
            Version::V4(d) => d,
            Version::V5(d) => d,
            Version::Crossbeam(d) => d,
        };
        let runner = main_runner.spawn_runner(format!("version_{}", version_desc));
        for config in &configs {
            let config_desc = &config.name;
            main_runner.reset_ids();
            info!(
                "Starting {} tests with profile {}",
                version_desc, config_desc
            );
            let runner = runner.spawn_runner(format!("config_{}", config_desc));

            match v {
                Version::V1(_) => run_bench_1(&runner, V1Maker, config.clone()),
                Version::V2(_) => run_bench_1(&runner, V2Maker, config.clone()),
                Version::V3(_) => run_bench_1(&runner, V3Maker, config.clone()),
                Version::V4(_) => run_bench_1(&runner, V4Maker, config.clone()),
                Version::V5(_) => run_bench_1(&runner, V5Maker, config.clone()),
                Version::Crossbeam(_) => run_bench_1(&runner, CrossbeamMaker, config.clone()),
            }
            .context("failed to run benchmark 1")?;

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
        "Benchmarks completed. Ran for {}s",
        start.elapsed().as_secs_f64()
    );

    Ok(())
}

/// start and end are (tx_count, rx_count)
fn create_configs_ramping<T>(
    start: (usize, usize),
    end: (usize, usize),
    ttl_s: (f64, f64),
    make_payload: fn() -> T,
) -> Vec<test_1::Config<T>> {
    let size = std::mem::size_of::<T>();
    let mut configs = vec![];
    let mut tx = start.0;
    let mut rx = start.1;
    loop {
        let name = format!("{}_{}_{}_{}_{}", tx, rx, ttl_s.0, ttl_s.1, size);
        configs.push(test_1::Config {
            name,
            n_sendrs: tx,
            n_recvrs: rx,
            sendrs_ttl_s: Some(ttl_s.0),
            recvrs_ttl_s: Some(ttl_s.1),
            make_payload,
        });
        if tx == end.0 && rx == end.1 {
            break;
        }
        if tx != end.0 {
            tx += 1;
        }
        if rx != end.1 {
            rx += 1;
        }
    }

    configs
}

use std::{
    fs::{self, DirEntry, File},
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Mutex,
    time::Instant,
};

use anyhow::Context;
use bench::aggregate::Aggregation;
use log::debug;
use rayon::ThreadPoolBuilder;
use serde::{Deserialize, Serialize};

fn main() -> anyhow::Result<()> {
    let start = Instant::now();
    env_logger::builder()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Debug)
        .init();
    ThreadPoolBuilder::new()
        .num_threads(8)
        .build_global()
        .context("unable to build thread pool")?;
    let path = std::env::args()
        .nth(1)
        .ok_or(anyhow::Error::msg("expected a path argument"))?;

    let path = Path::new(&path).to_path_buf();

    if !fs::exists(&path).context(format!("could not check for the existence of {path:?}"))? {
        return Err(anyhow::Error::msg(format!("path {path:?} does not exist")));
    }

    let path = path.join("main_runner");

    if !fs::exists(&path).context(format!(
        "could not check the existence of main_runner in {path:?}"
    ))? {
        return Err(anyhow::Error::msg(format!(
            "no \"main_runner\" directory found in {:?}",
            path.parent()
        )));
    }

    let save_to_root = PathBuf::from_str("output/aggregation").unwrap();

    fs::create_dir_all(&save_to_root).context(format!(
        "could not create parent directory for {save_to_root:?}"
    ))?;

    let results = Mutex::new(vec![]);
    rayon::scope(|s| {
        for version_entry in fs::read_dir(&path)
            .context(format!("could not find directory {path:?}"))
            .unwrap()
        {
            let version_entry = version_entry.unwrap();
            let version_path = version_entry.path();
            if version_path.is_dir() {
                let version_name = version_path
                    .file_name()
                    .context(format!("path {version_path:?} is invalid"))
                    .unwrap()
                    .to_str()
                    .ok_or(anyhow::Error::msg(format!(
                        "could not convert path {version_path:?} to string"
                    )))
                    .unwrap()
                    .replace("version_", "");

                for config_entry in version_path.read_dir().unwrap() {
                    let config_entry = config_entry.unwrap();
                    let version_name = version_name.clone();
                    if config_entry.path().is_dir() {
                        s.spawn(|_| {
                            results
                                .lock()
                                .unwrap()
                                .push(run_work(config_entry, version_name))
                        });
                    }
                }
            }
        }
    });
    let results = results.into_inner().unwrap();

    let mut global_bp_max = f64::MIN;
    let mut global_tp_max = f64::MIN;
    let mut global_lat_max = f64::MIN;

    for result in results {
        let run = result?;
        global_bp_max = global_bp_max.max(run.summary.max_backpressure);
        global_tp_max = global_tp_max.max(run.summary.max_throughput);
        global_lat_max = global_lat_max.max(run.summary.latency_max);
        global_lat_max = global_lat_max.max(run.summary.recv_max);
        global_lat_max = global_lat_max.max(run.summary.send_max);

        let save_to = save_to_root.join(format!("{}_{}.json", run.version, run.config));
        File::create(&save_to)?.write_all(&serde_json::to_vec(&run)?)?;
        debug!("wrote run result to {:?}", save_to);
    }

    File::create(save_to_root.join("summary.json"))?.write_all(&serde_json::to_vec(&Summary {
        global_bp_max,
        global_tp_max,
        global_lat_max,
    })?)?;

    debug!("ran aggregation in {:.2}s", start.elapsed().as_secs_f64());
    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
struct Summary {
    global_bp_max: f64,
    global_tp_max: f64,
    global_lat_max: f64,
}

fn run_work(config_entry: DirEntry, version_name: String) -> anyhow::Result<Run> {
    let config_path = config_entry.path();
    if !config_path.is_dir() {
        Err(anyhow::Error::msg("not a valid directory"))
    } else {
        let config_path_str = config_path
            .file_name()
            .context(format!("path {config_path:?} is invalid"))?
            .to_str()
            .ok_or(anyhow::Error::msg(format!(
                "could not convert path {config_path:?} to string"
            )))?;
        let config_name = config_path_str.replace("config_", "");

        let summary = Aggregation::from_directory(&config_path, 0.1).context(format!(
            "could not run aggregation for version \"{}\" config \"{}\"",
            version_name, config_name
        ))?;

        let result = Run {
            version: version_name.clone(),
            config: config_name,
            summary,
        };

        Ok(result)
    }
}

#[derive(Serialize, Deserialize)]
struct Run {
    version: String,
    config: String,
    summary: Aggregation,
}

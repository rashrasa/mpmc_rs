use std::{
    fs::{self, DirEntry, File},
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Mutex,
    time::Instant,
};

use anyhow::Context;
use bench::aggregate::{Aggregation, DistributionSummary, GaugeSummary};
use log::{debug, error, info};
use rayon::ThreadPoolBuilder;
use serde::{Deserialize, Serialize};

fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Debug)
        .init();
    ThreadPoolBuilder::new()
        .build_global()
        .context("unable to build thread pool")?;
    let path = std::env::args()
        .nth(1)
        .ok_or(anyhow::Error::msg("expected a path argument"))?;

    if std::env::args()
        .find(|v| v.eq_ignore_ascii_case("unsafe"))
        .is_none()
    {
        let mut answer = String::new();
        println!(
            "\n\n\n\x1b[31mPlease confirm that all files in {} will not be accessed for the duration of the run.",
            path
        );
        println!(
            "This application uses file memory-mapping and modifying files while it's running may result in undefined behaviour."
        );
        print!("Do you confirm (Y/n)?: \x1b[0m");
        std::io::stdout().flush()?;

        std::io::stdin()
            .read_line(&mut answer)
            .expect("Invalid input.");
        if !answer.to_ascii_lowercase().starts_with("y") {
            return Ok(());
        }
    }
    info!("started");
    let start = Instant::now();
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
                            // Safety: We printed a bright red warning that all files in this config's directory
                            // is to be accessed and the user has provided a confirmation.
                            let run = unsafe { run_work(config_entry, version_name) }.unwrap();
                            let save_to =
                                save_to_root.join(format!("{}_{}.json", run.version, run.config));
                            debug!("wrote run result to {:?}", save_to);
                            File::create(&save_to)
                                .unwrap()
                                .write_all(&serde_json::to_vec(&run).unwrap())
                                .unwrap();
                            results.lock().unwrap().push(run);
                        });
                    }
                }
            }
        }
    });
    let results = match results.into_inner() {
        Ok(v) => v,
        Err(e) => {
            error!("{:?}", e);
            e.into_inner()
        }
    };

    let mut summaries = vec![];
    for run in results {
        summaries.push((&run).into());
    }
    rayon::spawn(move || {
        File::create(save_to_root.join("summary.json"))
            .unwrap()
            .write_all(&serde_json::to_vec(&Summary { summaries }).unwrap())
            .unwrap()
    });

    debug!("ran aggregation in {:.2}s", start.elapsed().as_secs_f64());
    Ok(())
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct Summary {
    summaries: Vec<RunSummary>,
}

/// This data is extremely specific to plotting the data
/// and may not accurately reflect the field names.
#[derive(Serialize, Deserialize, Debug)]
struct RunSummary {
    version: String,
    config: String,
    backpressure: MetricSummary,
    throughput: MetricSummary,
    latency: MetricSummary,
    recv: MetricSummary,
    send: MetricSummary,
}

impl From<&Run> for RunSummary {
    fn from(run: &Run) -> Self {
        Self {
            version: run.version.clone(),
            config: run.config.clone(),
            backpressure: (&run.summary.backpressure).into(),
            throughput: (&run.summary.throughput).into(),
            latency: (&run.summary.latency).into(),
            recv: (&run.summary.recv).into(),
            send: (&run.summary.send).into(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct MetricSummary {
    max: f64,
    mean: f64,
    count: f64,

    p50: Option<f64>,
    p99: Option<f64>,
    p999: Option<f64>,
}

impl From<&GaugeSummary> for MetricSummary {
    fn from(summary: &GaugeSummary) -> Self {
        Self {
            max: summary.max,
            mean: summary.values.iter().sum::<f64>() / summary.values.len() as f64,
            count: summary.values.len() as f64,
            p50: None,
            p99: None,
            p999: None,
        }
    }
}

impl From<&DistributionSummary> for MetricSummary {
    fn from(summary: &DistributionSummary) -> Self {
        Self {
            max: summary.max,
            mean: summary.mean,
            count: summary.count as f64,
            p50: Some(summary.overall_p50),
            p99: Some(summary.overall_p99),
            p999: Some(summary.overall_p999),
        }
    }
}

/// # Safety
///
/// No other processes should be able to access any file in this config's directory.
unsafe fn run_work(config_entry: DirEntry, version_name: String) -> anyhow::Result<Run> {
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

        let summary = unsafe {
            Aggregation::from_directory(
                &config_path,
                format!(
                    "{}.bin",
                    config_path
                        .clone()
                        .into_string()
                        .map_err(|_| anyhow::Error::msg(format!(
                            "could not transform {:?} into string",
                            config_path
                        )))?
                ),
                0.25,
            )
        }
        .context(format!(
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

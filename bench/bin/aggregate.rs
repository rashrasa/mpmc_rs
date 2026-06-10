use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;
use bench::aggregate::Aggregation;
use log::info;
use serde::{Deserialize, Serialize};

fn main() -> anyhow::Result<()> {
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

    let save_to_root = PathBuf::from("output/aggregation");

    fs::create_dir_all(&save_to_root)
        .context(format!("could not create directory {save_to_root:?}"))?;

    for version_entry in
        fs::read_dir(&path).context(format!("could not find directory {path:?}"))?
    {
        let version_entry = version_entry?;
        let version_path = version_entry.path();
        if version_path.is_dir() {
            let version_name = String::from(
                version_path
                    .file_name()
                    .context(format!("path {version_path:?} is invalid"))?
                    .to_str()
                    .ok_or(anyhow::Error::msg(format!(
                        "could not convert path {version_path:?} to string"
                    )))?
                    .replace("version_", ""),
            );

            for config_entry in version_path.read_dir().unwrap() {
                let config_entry = config_entry?;
                let config_path = config_entry.path();
                if config_path.is_dir() {
                    let config_path_str = config_path
                        .file_name()
                        .context(format!("path {config_path:?} is invalid"))?
                        .to_str()
                        .ok_or(anyhow::Error::msg(format!(
                            "could not convert path {config_path:?} to string"
                        )))?;
                    let config_name = config_path_str.replace("config_", "");

                    let agg = Aggregation::from_directory(&config_path, 0.001).context(format!(
                        "could not run aggregation for version \"{}\" config \"{}\"",
                        version_name, config_name
                    ))?;

                    let save_to =
                        save_to_root.join(format!("{}_{}.json", version_name, config_name));

                    let mut file =
                        BufWriter::new(fs::File::create(&save_to).context("could not open file")?);

                    let result = Run {
                        version: version_name.clone(),
                        config: config_name,
                        aggregation: agg,
                    };

                    file.write_all(&serde_json::to_vec_pretty(&result)?)?;

                    info!("wrote result to {}", save_to.to_str().unwrap());
                }
            }
        }
    }

    return Ok(());
}

#[derive(Serialize, Deserialize)]
struct Run {
    version: String,
    config: String,
    aggregation: Aggregation,
}

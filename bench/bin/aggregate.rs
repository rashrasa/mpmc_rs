use std::{
    fs::File,
    io::{BufWriter, Write},
};

use anyhow::Context;
use bench::aggregate::Aggregation;
use log::info;

fn main() -> anyhow::Result<()> {
    let save_to = "aggregation.txt";
    let agg =
        Aggregation::from_directory("./results/main_runner/version_v1_naive/config_3_3_10_10_4")
            .context("could not run aggregation")?;

    let mut file = BufWriter::new(File::create(save_to).context("could not open file")?);

    file.write_all(&format!("{}", agg).into_bytes())?;

    info!("wrote results to {}", save_to);

    return Ok(());
}

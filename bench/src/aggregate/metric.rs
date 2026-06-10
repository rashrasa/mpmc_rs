use anyhow::Context;

/// Calculates the `p*100`-th percentile of `sorted_values`.
///
/// Values must already be sorted.
/// `p` must be in `[0.0, 1.0]`.
///
/// Uses linear interpolation when between two values.
pub fn percentile(sorted_values: &[f64], p: f64) -> anyhow::Result<f64> {
    assert!(
        p >= 0.0 && p <= 1.0,
        "percentile only accepts p in range [0.0, 1.0]"
    );

    let n = sorted_values.len();

    let target = p * (n - 1) as f64;

    let low = target.floor();
    let high = target.ceil();
    let weight = target - low;
    let low_idx = low as usize;

    let low_val = sorted_values
        .get(low_idx)
        .ok_or(anyhow::Error::msg(format!(
            "index {} out of bounds. length: {}",
            low_idx, n
        )))?;

    let high_idx = (high as usize).min(n - 1);
    let high_val = sorted_values
        .get(high_idx)
        .ok_or(anyhow::Error::msg(format!(
            "index {} out of bounds. length: {}",
            high_idx, n
        )))?;

    Ok(low_val * (1.0 - weight) + high_val * (weight))
}

pub struct Metric {
    pub count: usize,

    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub std_dev: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub p999: f64,
}

pub struct LazyWindowedMetric {
    n_buckets: usize,
    period: f64,
    start: f64,
    end: f64,
    buckets: Vec<LazyWindowedMetricBucket>,
}

#[derive(Clone)]
pub struct LazyWindowedMetricBucket {
    start: f64,
    end: f64,
    sorted_values: Vec<f64>,
}

impl LazyWindowedMetric {
    pub fn new(period: f64, start: f64, end: f64) -> Self {
        let n = ((end - start) / period).ceil() as usize;
        Self {
            period,
            n_buckets: n,
            start,
            end,
            buckets: vec![
                LazyWindowedMetricBucket {
                    start: start,
                    end: end,
                    sorted_values: vec![]
                };
                n
            ],
        }
    }

    // `time` must be in `[start, end]`
    pub fn add(&mut self, value: f64, time: f64) -> anyhow::Result<()> {
        assert!(
            time >= self.start && time <= self.end,
            "time not in range [start, end]",
        );
        let bucket = ((time - self.start) / self.period).floor() as usize;

        let dst_i = match self
            .buckets
            .get(bucket)
            .ok_or(anyhow::Error::msg(format!(
                "index {bucket} out of range. bucket count: {}",
                self.n_buckets
            )))?
            .sorted_values
            .binary_search_by(|a| a.total_cmp(&value))
        {
            Ok(i) => i,
            Err(i) => i,
        };
        self.buckets[bucket].sorted_values.insert(dst_i, value);
        Ok(())
    }

    pub fn generate(&self) -> anyhow::Result<Vec<Metric>> {
        let mut result = vec![];
        for bucket in &self.buckets {
            if bucket.sorted_values.len() == 0 {
                result.push(Metric {
                    count: 0,

                    min: f64::NAN,
                    max: f64::NAN,
                    mean: f64::NAN,
                    std_dev: f64::NAN,
                    p50: f64::NAN,
                    p90: f64::NAN,
                    p95: f64::NAN,
                    p99: f64::NAN,
                    p999: f64::NAN,
                });
                continue;
            }
            let mut min = f64::INFINITY;
            let mut max = -f64::INFINITY;

            let res = std_dev::standard_deviation(&bucket.sorted_values);

            let mean = res.mean;
            let std_dev = res.standard_deviation;

            for v in bucket.sorted_values.iter() {
                min = min.min(*v);
                max = max.max(*v);
            }

            result.push(Metric {
                count: bucket.sorted_values.len(),

                min,
                max,
                mean,
                std_dev,
                p50: percentile(&bucket.sorted_values, 0.5).context("failed to calculate p50")?,
                p90: percentile(&bucket.sorted_values, 0.9).context("failed to calculate p90")?,
                p95: percentile(&bucket.sorted_values, 0.95).context("failed to calculate p95")?,
                p99: percentile(&bucket.sorted_values, 0.99).context("failed to calculate p99")?,
                p999: percentile(&bucket.sorted_values, 0.999)
                    .context("failed to calculate p999")?,
            });
        }
        Ok(result)
    }

    pub fn n_buckets(&self) -> usize {
        self.n_buckets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    const F64_ACCEPTABLE_ERROR: f64 = 1.0e-12;

    #[test]
    fn percentile_median_even() {
        let items: [f64; _] = [1., 2., 3., 4., 5., 6., 7., 8., 9., 10.];

        assert_relative_eq!(
            5.5,
            percentile(&items, 0.5).unwrap(),
            epsilon = F64_ACCEPTABLE_ERROR
        );
    }

    #[test]
    fn percentile_median_odd() {
        let items: [f64; _] = [1., 2., 3., 4., 5., 6., 7., 8., 9.];

        assert_relative_eq!(
            5.,
            percentile(&items, 0.5).unwrap(),
            epsilon = F64_ACCEPTABLE_ERROR
        );
    }

    #[test]
    fn percentile_p99() {
        let items: [f64; _] = [1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 199.];

        assert_relative_eq!(
            178.32,
            percentile(&items, 0.99).unwrap(),
            epsilon = F64_ACCEPTABLE_ERROR
        );
    }

    /// result calculated from matlab
    ///
    /// ```matlab
    /// digits(64)
    /// V = transpose([0:99999]);
    /// C = times(V,V);
    /// a = prctile(C, 99.9, Method="inclusive");
    /// sprintf('%0.12f', a)
    /// ```
    #[test]
    fn percentile_p999_large_list() {
        let items: Vec<f64> = (0..=99_999).map(|i| i as f64 * i as f64).collect();

        assert_relative_eq!(
            9_979_810_400.798_999_786_337,
            percentile(&items, 0.999).unwrap(),
            epsilon = F64_ACCEPTABLE_ERROR
        );
    }

    #[test]
    fn lazy_windowed_metric_correct_n_buckets() {
        let metric = LazyWindowedMetric::new(250.0, 0.0, 1001.0);
        assert_eq!(5, metric.n_buckets);
    }

    #[test]
    fn lazy_windowed_metric_generate_correct() {
        let mut metric = LazyWindowedMetric::new(250.0, 0.0, 1000.0);
        assert_eq!(4, metric.n_buckets);

        metric.add(1.0, 0.0).unwrap();
        metric.add(4.0, 100.0).unwrap();
        metric.add(8.0, 150.0).unwrap();
        metric.add(9.0, 249.999_999_999_999).unwrap();

        let result = &metric.generate().unwrap()[0];

        assert_eq!(4, result.count);
        assert_relative_eq!(5.5, result.mean, epsilon = F64_ACCEPTABLE_ERROR);
        assert_relative_eq!(6.0, result.p50, epsilon = F64_ACCEPTABLE_ERROR);
        assert_relative_eq!(8.7, result.p90, epsilon = F64_ACCEPTABLE_ERROR);
        assert_relative_eq!(8.85, result.p95, epsilon = F64_ACCEPTABLE_ERROR);
        assert_relative_eq!(8.97, result.p99, epsilon = F64_ACCEPTABLE_ERROR);
        assert_relative_eq!(8.997, result.p999, epsilon = F64_ACCEPTABLE_ERROR);
    }
}

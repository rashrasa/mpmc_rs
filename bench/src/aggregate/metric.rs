use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};

use crate::aggregate::{DistributionSummary, GaugeSummary};

/// Calculates the `p*100`-th percentile of `sorted_values`.
///
/// Values must already be sorted.
/// `p` must be in `[0.0, 1.0]`.
///
/// Uses linear interpolation when between two values.
pub fn percentile(sorted_values: &[f64], p: f64) -> anyhow::Result<f64> {
    assert!(
        (0.0..=1.0).contains(&p),
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

#[derive(Serialize, Deserialize)]
pub enum DistributionMetric {
    NoEvents {
        start: f64,
        end: f64,
    },
    Distribution {
        start: f64,
        end: f64,
        count: usize,
        min: f64,
        max: f64,
        mean: f64,
        p50: f64,
        p90: f64,
        p95: f64,
        p99: f64,
        p999: f64,
        p9999: f64,
        p99999: f64,
        raw_values: Vec<f64>,
    },
}

#[derive(Serialize, Deserialize)]
pub enum GaugeMetric {
    NoEvents {
        start: f64,
        end: f64,
    },
    Gauge {
        start: f64,
        end: f64,
        count: usize,
        value: f64,
    },
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
    values: Vec<f64>,
}

impl LazyWindowedMetric {
    pub fn new(period: f64, start: f64, end: f64) -> Self {
        let n = (((end - start) / period).ceil() as usize).max(1);

        let mut buckets = Vec::with_capacity(n);
        let mut t = start;

        for _ in 0..n {
            buckets.push(LazyWindowedMetricBucket {
                start: t,
                end: t + period,
                values: Vec::with_capacity(1_000_000),
            });
            t += period;
        }

        Self {
            period,
            n_buckets: n,
            start,
            end,
            buckets,
        }
    }

    // `time` must be in `[start, end]`
    pub fn add(&mut self, value: f64, time: f64) -> anyhow::Result<()> {
        debug_assert!(
            time >= self.start && time <= self.end,
            "time not in range [{}, {}]",
            self.start,
            self.end
        );
        let bucket = if time == self.end {
            // only the end range of the end bucket is inclusive
            self.n_buckets - 1
        } else {
            ((time - self.start) / self.period).floor() as usize
        };

        self.buckets
            .get_mut(bucket)
            .ok_or_else(|| {
                anyhow!(
                    "index {bucket} out of range. bucket count: {}",
                    self.n_buckets
                )
            })?
            .values
            .push(value);
        Ok(())
    }

    /// Consumes self and creates a distribution.
    pub fn generate(self) -> anyhow::Result<DistributionSummary> {
        let mut t = vec![];
        let mut p50 = vec![];
        let mut p99 = vec![];
        let mut p999 = vec![];
        let mut max = f64::MIN;
        let mut sum = 0.0;
        let mut count = 0;
        let mut values = vec![];

        for mut bucket in self.buckets {
            if bucket.values.is_empty() {
                continue;
            }
            bucket.values.sort_unstable_by(f64::total_cmp);

            t.push(bucket.start);
            p50.push(percentile(&bucket.values, 0.5).context("failed to calculate p50")?);
            p99.push(percentile(&bucket.values, 0.99).context("failed to calculate p99")?);
            p999.push(percentile(&bucket.values, 0.999).context("failed to calculate p999")?);

            count += bucket.values.len();
            for v in bucket.values.iter() {
                let v = *v;
                max = max.max(v);
                sum += v;
            }
            values.extend(bucket.values)
        }
        values.sort_unstable_by(f64::total_cmp);
        let mean = sum / count as f64;
        let overall_p50 = percentile(&values, 0.5)?;
        let overall_p99 = percentile(&values, 0.5)?;
        let overall_p999 = percentile(&values, 0.5)?;
        Ok(DistributionSummary {
            t,
            mean,
            count,
            p50,
            overall_p50,
            p99,
            overall_p99,
            p999,
            overall_p999,
            max,
        })
    }

    /// Consumes self and creates a gauge summary.
    pub fn generate_gauged<F>(self, mut agg: F) -> anyhow::Result<GaugeSummary>
    where
        F: FnMut(std::slice::Iter<f64>, &f64, &f64) -> f64,
    {
        let mut values = vec![];
        let mut t = vec![];
        let mut max = f64::MIN;
        let mut sum = 0.0;
        let mut count = 0;
        for bucket in self.buckets {
            if bucket.values.is_empty() {
                continue;
            }

            let value = agg(bucket.values.iter(), &bucket.start, &bucket.end);
            values.push(value);
            t.push(bucket.start);
            sum += value;
            max = max.max(value);
            count += 1;
        }
        let mean = sum / count as f64;
        Ok(GaugeSummary {
            t,
            values,
            mean,
            max,
        })
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

        #[allow(clippy::excessive_precision)]
        let expected = 9_979_810_400.798_999_786_337;

        assert_relative_eq!(
            expected,
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
    fn lazy_windowed_metric_one_bucket_generate_correct() {
        let mut metric = LazyWindowedMetric::new(250.0, 0.0, 1000.0);
        assert_eq!(4, metric.n_buckets);

        metric.add(1.0, 0.0).unwrap();
        metric.add(4.0, 100.0).unwrap();
        metric.add(8.0, 150.0).unwrap();
        metric.add(9.0, 249.999_999_999_999).unwrap();

        let _result = &metric.generate().unwrap();
        todo!()
    }
}

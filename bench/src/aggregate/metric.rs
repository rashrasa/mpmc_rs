use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};

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
                values: vec![],
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

    pub fn generate(&mut self) -> anyhow::Result<Vec<DistributionMetric>> {
        let mut result = vec![];
        for bucket in &mut self.buckets {
            if bucket.values.is_empty() {
                result.push(DistributionMetric::NoEvents {
                    start: bucket.start,
                    end: bucket.end,
                });
                continue;
            }
            bucket.values.retain(|v| !v.is_nan());
            bucket.values.sort_unstable_by(|a, b| a.total_cmp(b));

            let mut min = f64::INFINITY;
            let mut max = -f64::INFINITY;

            let mean = bucket.values.iter().sum::<f64>() / bucket.values.len() as f64;

            for v in bucket.values.iter() {
                min = min.min(*v);
                max = max.max(*v);
            }

            result.push(DistributionMetric::Distribution {
                start: bucket.start,
                end: bucket.end,

                count: bucket.values.len(),

                min,
                max,
                mean,
                p50: percentile(&bucket.values, 0.5).context("failed to calculate p50")?,
                p90: percentile(&bucket.values, 0.9).context("failed to calculate p90")?,
                p95: percentile(&bucket.values, 0.95).context("failed to calculate p95")?,
                p99: percentile(&bucket.values, 0.99).context("failed to calculate p99")?,
                p999: percentile(&bucket.values, 0.999).context("failed to calculate p999")?,
                p9999: percentile(&bucket.values, 0.9999).context("failed to calculate p9999")?,
                p99999: percentile(&bucket.values, 0.99999)
                    .context("failed to calculate p99999")?,
                raw_values: bucket.values.clone(),
            });
        }
        Ok(result)
    }

    pub fn generate_gauged<F>(&mut self, mut agg: F) -> anyhow::Result<Vec<GaugeMetric>>
    where
        F: FnMut(std::slice::Iter<f64>, &f64, &f64) -> f64,
    {
        let mut result = vec![];
        for bucket in &mut self.buckets {
            if bucket.values.is_empty() {
                result.push(GaugeMetric::NoEvents {
                    start: bucket.start,
                    end: bucket.end,
                });
                continue;
            }
            bucket.values.retain(|v| !v.is_nan());

            let value = agg(bucket.values.iter(), &bucket.start, &bucket.end);

            result.push(GaugeMetric::Gauge {
                start: bucket.start,
                end: bucket.end,

                count: bucket.values.len(),

                value: value / self.period,
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

        let result = &metric.generate().unwrap()[0];

        match result {
            DistributionMetric::NoEvents { .. } => panic!("no events in this bucket"),
            DistributionMetric::Distribution {
                start,
                end,
                count,
                min,
                max,
                mean,
                p50,
                p90,
                p95,
                p99,
                p999,
                p9999,
                p99999,
                raw_values,
            } => {
                assert_relative_eq!(1.0, raw_values[0]);
                assert_relative_eq!(4.0, raw_values[1]);
                assert_relative_eq!(8.0, raw_values[2]);
                assert_relative_eq!(9.0, raw_values[3]);

                assert_eq!(4, *count);
                assert_relative_eq!(0.0, *start, epsilon = F64_ACCEPTABLE_ERROR);
                assert_relative_eq!(250.0, *end, epsilon = F64_ACCEPTABLE_ERROR);

                assert_relative_eq!(1.0, *min, epsilon = F64_ACCEPTABLE_ERROR);
                assert_relative_eq!(9.0, *max, epsilon = F64_ACCEPTABLE_ERROR);

                assert_relative_eq!(5.5, *mean, epsilon = F64_ACCEPTABLE_ERROR);

                assert_relative_eq!(6.0, *p50, epsilon = F64_ACCEPTABLE_ERROR);
                assert_relative_eq!(8.7, *p90, epsilon = F64_ACCEPTABLE_ERROR);
                assert_relative_eq!(8.85, *p95, epsilon = F64_ACCEPTABLE_ERROR);
                assert_relative_eq!(8.97, *p99, epsilon = F64_ACCEPTABLE_ERROR);
                assert_relative_eq!(8.997, *p999, epsilon = F64_ACCEPTABLE_ERROR);
                assert_relative_eq!(8.9997, *p9999, epsilon = F64_ACCEPTABLE_ERROR);
                assert_relative_eq!(8.99997, *p99999, epsilon = F64_ACCEPTABLE_ERROR);
            }
        }
    }
}

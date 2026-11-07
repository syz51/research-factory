//! The tests `lab evaluate` uses: an exact stratified hypergeometric test for
//! per-session indicators, and the van Elteren stratified Wilcoxon rank-sum
//! test for numeric session fields.
//!
//! Both answer one question per lesson: is the after-adoption group lower
//! (`p_lower`) or higher (`p_upper`) than the before group would make likely
//! by chance, comparing like with like within each stratum?

/// A discrete distribution on the integers `offset ..= offset + pmf.len() - 1`.
#[derive(Debug, Clone)]
struct Dist {
    offset: u64,
    pmf: Vec<f64>,
}

impl Dist {
    fn cdf_le(&self, t: u64) -> f64 {
        if t < self.offset {
            return 0.0;
        }
        let end = ((t - self.offset) as usize + 1).min(self.pmf.len());
        self.pmf[..end].iter().sum::<f64>().min(1.0)
    }

    fn sf_ge(&self, t: u64) -> f64 {
        let start = t.saturating_sub(self.offset) as usize;
        if start >= self.pmf.len() {
            return 0.0;
        }
        self.pmf[start..].iter().sum::<f64>().min(1.0)
    }

    fn convolve(&self, other: &Dist) -> Dist {
        let mut pmf = vec![0.0; self.pmf.len() + other.pmf.len() - 1];
        for (i, &a) in self.pmf.iter().enumerate() {
            if a == 0.0 {
                continue;
            }
            for (j, &b) in other.pmf.iter().enumerate() {
                pmf[i + j] += a * b;
            }
        }
        Dist {
            offset: self.offset + other.offset,
            pmf,
        }
    }
}

/// The pmf of X ~ Hypergeometric(population `total`, `successes`, `draws`).
///
/// Built from the ratio of neighbouring probabilities outward from the mode
/// and normalised by their sum, so no factorial of a large number is formed
/// and the relative error stays near machine precision; probabilities below
/// the smallest double relative to the mode become zero.
fn hypergeometric(total: u64, successes: u64, draws: u64) -> Dist {
    debug_assert!(successes <= total && draws <= total);
    let lo = draws.saturating_sub(total - successes);
    let hi = draws.min(successes);
    let (n, k, d) = (total as f64, successes as f64, draws as f64);
    let mode = (((draws + 1) as u128 * (successes + 1) as u128 / (total + 2) as u128) as u64)
        .clamp(lo, hi);
    let mut w = vec![0.0; (hi - lo + 1) as usize];
    let at = |x: u64| (x - lo) as usize;
    w[at(mode)] = 1.0;
    // P(x + 1) / P(x) = (K - x)(n - x) / ((x + 1)(N - K - n + x + 1)).
    for x in mode..hi {
        let xf = x as f64;
        let r = (k - xf) * (d - xf) / ((xf + 1.0) * (n - k - d + xf + 1.0));
        w[at(x + 1)] = w[at(x)] * r;
    }
    for x in (lo + 1..=mode).rev() {
        let xf = x as f64;
        let r = xf * (n - k - d + xf) / ((k - xf + 1.0) * (d - xf + 1.0));
        w[at(x - 1)] = w[at(x)] * r;
    }
    let sum: f64 = w.iter().sum();
    for v in &mut w {
        *v /= sum;
    }
    Dist { offset: lo, pmf: w }
}

/// One stratum of per-session indicators: sessions and positives before and after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub before: u64,
    pub before_positive: u64,
    pub after: u64,
    pub after_positive: u64,
}

/// One-sided p-values of the exact stratified conditional test.
///
/// In each stratum, given its group sizes and its total positives, the
/// after-group positives are hypergeometric under the null hypothesis that
/// adoption changed nothing. Their sum over strata has the convolution of
/// those distributions; `p_lower = P(T ≤ t_obs)`, `p_upper = P(T ≥ t_obs)`.
pub fn stratified_hypergeometric(strata: &[Counts]) -> (f64, f64) {
    let mut dist = Dist {
        offset: 0,
        pmf: vec![1.0],
    };
    let mut observed = 0;
    for s in strata {
        let total = s.before + s.after;
        let positives = s.before_positive + s.after_positive;
        dist = dist.convolve(&hypergeometric(total, positives, s.after));
        observed += s.after_positive;
    }
    (dist.cdf_le(observed), dist.sf_ge(observed))
}

/// The unstratified `p_lower` after `extra` more after-sessions with no positive.
fn p_lower_with_extra(c: Counts, extra: u64) -> f64 {
    let total = c.before + c.after + extra;
    hypergeometric(total, c.before_positive + c.after_positive, c.after + extra)
        .cdf_le(c.after_positive)
}

/// The smallest number of additional after-sessions, all without a positive,
/// that would bring the pooled one-sided test below `threshold`; `None` when
/// even `limit` would not. An estimate of how long a lesson needs: the
/// stratified test and real sessions can differ.
pub fn sessions_needed(pooled: Counts, threshold: f64, limit: u64) -> Option<u64> {
    if p_lower_with_extra(pooled, 0) < threshold {
        return Some(0);
    }
    // p_lower falls as clean sessions are added; find a bracket, then bisect.
    let mut hi = 1;
    while p_lower_with_extra(pooled, hi) >= threshold {
        if hi >= limit {
            return None;
        }
        hi = (hi * 2).min(limit);
    }
    let mut lo = hi / 2; // p_lower(lo) >= threshold (or lo == 0, checked above)
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if p_lower_with_extra(pooled, mid) < threshold {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(hi)
}

/// Values of one stratum before and after adoption.
#[derive(Debug, Clone, Default)]
pub struct Samples {
    pub before: Vec<f64>,
    pub after: Vec<f64>,
}

/// The van Elteren statistic: `Z` and its one-sided p-values, or `None` when
/// every stratum's values are tied (no variance).
///
/// Per stratum, the pooled values get average ranks; `W` is the after
/// group's rank sum with mean `n_a(N+1)/2` and tie-corrected variance
/// `n_a n_b / 12 × ((N+1) − Σ(t³−t)/(N(N−1)))`. Strata are weighted by
/// `1/(N+1)`. The normal approximation has no continuity correction.
pub fn van_elteren(strata: &[Samples]) -> Option<(f64, f64, f64)> {
    let (mut num, mut var) = (0.0, 0.0);
    for s in strata {
        let (nb, na) = (s.before.len(), s.after.len());
        if nb == 0 || na == 0 {
            continue;
        }
        let n = (nb + na) as f64;
        let mut pooled: Vec<(f64, bool)> = s
            .before
            .iter()
            .map(|&v| (v, false))
            .chain(s.after.iter().map(|&v| (v, true)))
            .collect();
        pooled.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut w, mut ties) = (0.0, 0.0);
        let mut i = 0;
        while i < pooled.len() {
            let mut j = i;
            while j + 1 < pooled.len() && pooled[j + 1].0 == pooled[i].0 {
                j += 1;
            }
            // Ranks i+1 ..= j+1 share their average.
            let rank = (i + j + 2) as f64 / 2.0;
            let after = pooled[i..=j].iter().filter(|p| p.1).count() as f64;
            w += rank * after;
            let t = (j - i + 1) as f64;
            ties += t * t * t - t;
            i = j + 1;
        }
        let (naf, nbf) = (na as f64, nb as f64);
        let mean = naf * (n + 1.0) / 2.0;
        let v = naf * nbf / 12.0 * ((n + 1.0) - ties / (n * (n - 1.0)));
        let weight = 1.0 / (n + 1.0);
        num += weight * (w - mean);
        var += weight * weight * v;
    }
    if var <= 0.0 {
        return None;
    }
    let z = num / var.sqrt();
    Some((z, normal_cdf(z), normal_cdf(-z)))
}

/// The standard normal distribution function, from `erfc`.
pub fn normal_cdf(z: f64) -> f64 {
    0.5 * erfc(-z / std::f64::consts::SQRT_2)
}

/// The complementary error function, with relative error near 1e-13 or better.
///
/// Below 3 it is `1 − erf(x)` from the series
/// `erf(x) = 2/√π · e^{−x²} Σ 2ⁿ x^{2n+1} / (1·3·…·(2n+1))`, whose terms are
/// all positive; from 3 up it is the continued fraction
/// `erfc(x) = e^{−x²}/√π · 1/(x + (1/2)/(x + 1/(x + (3/2)/(x + …))))`,
/// evaluated with the modified Lentz method.
pub fn erfc(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x < 0.0 {
        return 2.0 - erfc(-x);
    }
    let frac_1_sqrt_pi = std::f64::consts::FRAC_2_SQRT_PI / 2.0;
    if x < 3.0 {
        let x2 = x * x;
        let (mut term, mut sum) = (x, x);
        let mut n = 0.0;
        loop {
            term *= 2.0 * x2 / (2.0 * n + 3.0);
            sum += term;
            n += 1.0;
            if term <= sum * 1e-17 {
                break;
            }
        }
        return 1.0 - 2.0 * frac_1_sqrt_pi * (-x2).exp() * sum;
    }
    if x > 27.3 {
        return 0.0; // below the smallest positive double
    }
    let tiny = 1e-300;
    let mut f = x;
    let (mut c, mut d) = (f, 0.0);
    for k in 1..1000 {
        let a = k as f64 / 2.0;
        d = x + a * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = x + a / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let delta = c * d;
        f *= delta;
        if (delta - 1.0).abs() < 1e-16 {
            break;
        }
    }
    frac_1_sqrt_pi * (-x * x).exp() / f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn unstratified_matches_scipy() {
        let (lo, up) = stratified_hypergeometric(&[Counts {
            before: 1000,
            before_positive: 10,
            after: 50,
            after_positive: 0,
        }]);
        assert!(close(lo, 0.612590961063294, 1e-9), "{lo}");
        assert!(close(up, 1.0, 1e-9), "{up}");
    }

    #[test]
    fn stratified_matches_scipy() {
        let (lo, up) = stratified_hypergeometric(&[
            Counts {
                before: 100,
                before_positive: 10,
                after: 20,
                after_positive: 0,
            },
            Counts {
                before: 40,
                before_positive: 2,
                after: 30,
                after_positive: 6,
            },
        ]);
        assert!(close(lo, 0.7935927405628815, 1e-9), "{lo}");
        assert!(close(up, 0.39732582580895653, 1e-9), "{up}");
    }

    #[test]
    fn large_strata_stay_normalised() {
        let (lo, up) = stratified_hypergeometric(&[Counts {
            before: 40_000,
            before_positive: 3_000,
            after: 30_000,
            after_positive: 1_900,
        }]);
        // scipy.stats.hypergeom(70000, 4900, 30000).cdf(1900)
        assert!(((lo - 1.0082248123691282e-09) / lo).abs() < 1e-9, "{lo}");
        assert!(close(up, 1.0, 1e-9), "{up}");
        let d = hypergeometric(70_000, 4_900, 30_000);
        assert!(close(d.pmf.iter().sum::<f64>(), 1.0, 1e-12));
    }

    #[test]
    fn single_stratum_wilcoxon_matches_scipy() {
        let s = Samples {
            before: vec![5., 7., 7., 9., 12., 15., 15., 20., 22., 30.],
            after: vec![3., 4., 7., 8., 9., 10., 11.],
        };
        let (_, lo, up) = van_elteren(&[s]).unwrap();
        assert!(close(lo, 0.03498618267703823, 1e-6), "{lo}");
        assert!(close(lo + up, 1.0, 1e-12));
        let tied = Samples {
            before: vec![1., 1.],
            after: vec![1.],
        };
        assert!(van_elteren(&[tied]).is_none());
    }

    #[test]
    fn normal_cdf_matches_reference_values() {
        // scipy.stats.norm.cdf
        for (z, p) in [
            (0.0, 0.5),
            (1.0, 0.8413447460685429),
            (-1.959963984540054, 0.024999999999999998),
            (-3.0, 0.001349898031630093),
            (-4.5, 3.3976731247300543e-06),
            (-5.0, 2.8665157187919344e-07),
            (-8.0, 6.22096057427174e-16),
            (-20.0, 2.753624118606155e-89),
            (2.5, 0.9937903346742238),
        ] {
            let got = normal_cdf(z);
            assert!(
                ((got - p) / p).abs() < 1e-10,
                "Φ({z}) = {got}, expected {p}"
            );
        }
    }

    #[test]
    fn sessions_needed_is_monotone_and_sane() {
        let c = |bp, a| Counts {
            before: 1000,
            before_positive: bp,
            after: a,
            after_positive: 0,
        };
        let m = sessions_needed(c(10, 12), 0.025, 1_000_000).unwrap();
        assert_eq!(m, 433, "scipy, by brute force");
        assert!(p_lower_with_extra(c(10, 12), m) < 0.025);
        assert!(p_lower_with_extra(c(10, 12), m - 1) >= 0.025);
        // More clean sessions already seen: fewer still needed.
        let m2 = sessions_needed(c(10, 200), 0.025, 1_000_000).unwrap();
        assert_eq!(m2, m - 188);
        // A more common finding needs fewer sessions.
        let common = sessions_needed(c(100, 12), 0.025, 1_000_000).unwrap();
        assert!(common < m, "{common} < {m}");
        assert_eq!(sessions_needed(c(0, 12), 0.025, 1_000_000), None);
        assert_eq!(sessions_needed(c(1, 12), 0.025, 100), None);
    }
}

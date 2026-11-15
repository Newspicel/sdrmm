use crate::math::special::gamma::{gamma_p, gamma_q, ln_gamma};

const DIRECT_SUM_LIMIT: f64 = 700.0;

pub(super) fn closed_pfa(looks: u32, shape: f64, ratio: f64) -> f64 {
    let ln_shape = ln_gamma(shape);
    let ln_ratio = ratio.ln();
    let ln_rise = ratio.ln_1p();
    (0..looks)
        .map(|term| {
            let i = f64::from(term);
            (ln_gamma(shape + i) - ln_gamma(i + 1.0) - ln_shape + power_ln(i, ln_ratio)
                - (shape + i) * ln_rise)
                .exp()
        })
        .sum()
}

pub(super) fn survival(looks: u32, y: f64) -> f64 {
    let x = f64::from(looks) * y;
    if x > DIRECT_SUM_LIMIT {
        return gamma_q(f64::from(looks), x);
    }
    let mut term = (-x).exp();
    let mut sum = term;
    for j in 1..looks {
        term *= x / f64::from(j);
        sum += term;
    }
    sum
}

pub(super) fn order_density(z: f64, cells: f64, order: f64, looks: f64, ln_choose: f64) -> f64 {
    let below = gamma_p(looks, looks * z);
    let above = gamma_q(looks, looks * z);
    (order.ln()
        + ln_choose
        + power_ln(order - 1.0, below.ln())
        + power_ln(cells - order, above.ln())
        + ln_unit_mean_gamma_pdf(looks, z))
    .exp()
}

pub(super) fn greatest_density(z: f64, shape: f64) -> f64 {
    2.0 * gamma_p(shape, shape * z) * ln_unit_mean_gamma_pdf(shape, z).exp()
}

pub(super) fn ln_unit_mean_gamma_pdf(shape: f64, z: f64) -> f64 {
    shape * shape.ln() + power_ln(shape - 1.0, z.ln()) - shape * z - ln_gamma(shape)
}

fn power_ln(exponent: f64, ln_value: f64) -> f64 {
    if exponent == 0.0 {
        0.0
    } else {
        exponent * ln_value
    }
}

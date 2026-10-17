//! Floating-point `printf` conversions (`%e %E %f %g %G`) as glibc produces
//! them, which DST's Linux build uses for every number it prints. Unlike
//! MSVCR90 (src/msvc_fmt.rs), glibc rounds the exact value once, ties to
//! even, prints exponents with at least two digits (`1e+20`) and spells
//! infinities and NaNs `inf` and `nan`.

use crate::msvc_fmt::Spec;

/// `n` significant digits of a positive finite value, correctly rounded
/// (ties to even, as glibc in the default rounding mode), and its decimal
/// exponent. Zero gives `n` zeros and exponent 0.
fn sci_digits(v: f64, n: usize) -> (String, i32) {
    if v == 0.0 {
        return ("0".repeat(n), 0);
    }
    let s = format!("{:.*e}", n - 1, v);
    let (mantissa, exp) = s.split_once('e').unwrap();
    (mantissa.chars().filter(char::is_ascii_digit).collect(), exp.parse().unwrap())
}

fn exponent(out: &mut String, upper: bool, e: i32) {
    out.push(if upper { 'E' } else { 'e' });
    out.push(if e < 0 { '-' } else { '+' });
    out.push_str(&format!("{:02}", e.unsigned_abs()));
}

fn fmt_e(v: f64, precision: usize, upper: bool, alt: bool) -> String {
    let (digits, e) = sci_digits(v, precision + 1);
    let mut s = String::new();
    s.push_str(&digits[..1]);
    if precision > 0 || alt {
        s.push('.');
    }
    s.push_str(&digits[1..]);
    exponent(&mut s, upper, e);
    s
}

fn fmt_f(v: f64, precision: usize, alt: bool) -> String {
    let mut s = format!("{:.*}", precision, v);
    if precision == 0 && alt {
        s.push('.');
    }
    s
}

fn fmt_g(v: f64, precision: usize, upper: bool, alt: bool) -> String {
    let p = precision.max(1);
    let (_, x) = sci_digits(v, p);
    let s = if (x as i64) < p as i64 && x >= -4 {
        fmt_f(v, (p as i32 - 1 - x) as usize, alt)
    } else {
        fmt_e(v, p - 1, upper, alt)
    };
    if alt {
        return s;
    }
    let (body, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], &s[i..]),
        None => (&s[..], ""),
    };
    let body = if body.contains('.') { body.trim_end_matches('0').trim_end_matches('.') } else { body };
    format!("{body}{exp}")
}

/// Formats `v` with one conversion specification, glibc style.
pub fn format(spec: &Spec, v: f64) -> String {
    let negative = v.is_sign_negative();
    let upper = spec.conv.is_ascii_uppercase();
    let a = v.abs();
    let precision = spec.precision.unwrap_or(6);
    let finite = a.is_finite();
    let body = if a.is_nan() {
        if upper { "NAN".to_string() } else { "nan".to_string() }
    } else if a.is_infinite() {
        if upper { "INF".to_string() } else { "inf".to_string() }
    } else {
        match spec.conv {
            b'e' | b'E' => fmt_e(a, precision, upper, spec.alt),
            b'f' => fmt_f(a, precision, spec.alt),
            _ => fmt_g(a, precision, upper, spec.alt),
        }
    };
    let sign = if negative {
        "-"
    } else if spec.plus {
        "+"
    } else if spec.space {
        " "
    } else {
        ""
    };
    let len = sign.len() + body.len();
    if len >= spec.width {
        return format!("{sign}{body}");
    }
    let pad = spec.width - len;
    if spec.left {
        format!("{sign}{body}{}", " ".repeat(pad))
    } else if spec.zero && finite {
        format!("{sign}{}{body}", "0".repeat(pad))
    } else {
        format!("{}{sign}{body}", " ".repeat(pad))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(spec: &str, v: f64) -> String {
        format(&Spec::parse(spec.as_bytes()).unwrap(), v)
    }

    #[test]
    fn two_digit_exponents() {
        assert_eq!(f("%.14g", 9.1844232182219e-05), "9.1844232182219e-05");
        assert_eq!(f("%.14g", 1e20), "1e+20");
        assert_eq!(f("%.14g", 1e100), "1e+100");
        assert_eq!(f("%e", 0.0), "0.000000e+00");
    }

    #[test]
    fn plain_values() {
        assert_eq!(f("%.14g", 0.1), "0.1");
        assert_eq!(f("%.14g", 100.0), "100");
        assert_eq!(f("%.14g", -2.5), "-2.5");
        assert_eq!(f("%.14g", 0.0001), "0.0001");
        assert_eq!(f("%.14g", 0.00001), "1e-05");
        assert_eq!(f("%.14g", 123456789012345.0), "1.2345678901234e+14");
        assert_eq!(f("%.2f", 3.14159), "3.14");
        assert_eq!(f("%5.1f", 2.25), "  2.2");
        assert_eq!(f("%.0f", 0.5), "0");
        assert_eq!(f("%.0f", 2.5), "2");
        assert_eq!(f("%.14g", 0.0), "0");
        assert_eq!(f("%.14g", -0.0), "-0");
    }

    #[test]
    fn special_values() {
        assert_eq!(f("%.14g", f64::INFINITY), "inf");
        assert_eq!(f("%.14g", f64::NEG_INFINITY), "-inf");
        assert_eq!(f("%.14g", f64::from_bits(0xfff8_0000_0000_0000)), "-nan");
        assert_eq!(f("%05.1f", f64::INFINITY), "  inf");
    }
}

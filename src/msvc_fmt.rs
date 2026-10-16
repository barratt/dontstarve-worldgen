//! Floating-point `printf` conversions (`%e %E %f %g %G`) as the Visual C++
//! 2008 runtime (MSVCR90) produces them, which both games use for every
//! number they print: `tostring`, `string.format` and the savedata.
//!
//! MSVCR90 differs from a correctly rounded `printf` in three ways, all
//! reproduced here:
//!
//! - it first converts the value to 17 significant digits, correctly
//!   rounded except that an exact tie usually rounds down (the runtime
//!   works in extended precision, so a few exact ties round up), and then rounds that
//!   digit string half-up to the requested precision, so a value just below
//!   a rounding boundary can round up ("double rounding");
//! - digits past the 17th are printed as zeros;
//! - exponents have at least three digits (`1e+020`), and infinities and
//!   NaNs print as `1.#INF`, `-1.#IND` (the default NaN), `1.#QNAN` or
//!   `1.#SNAN`, formatted like digit strings, so precision and the
//!   exponent of `%e` apply to them too.
//!
//! This is a reimplementation from observed behaviour, checked against the
//! real runtime on random inputs; no runtime code is used.

use std::ffi::{c_char, c_int, CStr};

/// A parsed conversion specification: `%[flags][width][.precision]conv`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spec {
    pub left: bool,
    pub plus: bool,
    pub space: bool,
    pub alt: bool,
    pub zero: bool,
    pub width: usize,
    pub precision: Option<usize>,
    pub conv: u8,
}

impl Spec {
    /// Parses a single specification such as `%.14g` or `%-08.3f`.
    pub fn parse(s: &[u8]) -> Option<Spec> {
        let mut spec = Spec::default();
        let mut i = 0;
        if s.first() != Some(&b'%') {
            return None;
        }
        i += 1;
        while let Some(&c) = s.get(i) {
            match c {
                b'-' => spec.left = true,
                b'+' => spec.plus = true,
                b' ' => spec.space = true,
                b'#' => spec.alt = true,
                b'0' => spec.zero = true,
                _ => break,
            }
            i += 1;
        }
        while let Some(&c) = s.get(i).filter(|c| c.is_ascii_digit()) {
            spec.width = spec.width * 10 + (c - b'0') as usize;
            i += 1;
        }
        if s.get(i) == Some(&b'.') {
            i += 1;
            let mut p = 0;
            while let Some(&c) = s.get(i).filter(|c| c.is_ascii_digit()) {
                p = p * 10 + (c - b'0') as usize;
                i += 1;
            }
            spec.precision = Some(p);
        }
        // Length modifiers do not change a double conversion.
        while matches!(s.get(i), Some(b'l' | b'L' | b'h')) {
            i += 1;
        }
        let conv = *s.get(i)?;
        if !matches!(conv, b'e' | b'E' | b'f' | b'g' | b'G') || i + 1 != s.len() {
            return None;
        }
        spec.conv = conv;
        Some(spec)
    }
}

/// Decimal digits of a value: `0.d1d2d3... x 10^decpt`.
struct Digits {
    digits: Vec<u8>,
    decpt: i32,
    negative: bool,
    /// Infinity/NaN: `digits` holds the text ("1#INF") and decpt is 1.
    special: bool,
}

/// The 17 correctly rounded significant digits MSVCR90 starts from.
fn digits17(v: f64) -> Digits {
    let negative = v.is_sign_negative();
    if !v.is_finite() {
        // MSVCR90 prints these as the digit strings below, with the point
        // after the first character ("1.#INF"); precision rounds them.
        let quiet = v.to_bits() & (1 << 51) != 0;
        let text: &[u8] = if v.is_infinite() {
            b"1#INF"
        } else if negative && is_default_nan(v) {
            b"1#IND"
        } else if quiet {
            b"1#QNAN"
        } else {
            b"1#SNAN"
        };
        return Digits { digits: text.to_vec(), decpt: 1, negative, special: true };
    }
    if v == 0.0 {
        return Digits { digits: vec![b'0'; 17], decpt: 0, negative, special: false };
    }
    // Rust formats exactly: {:.16e} is the correctly rounded 17-digit form
    // (ties to even). The runtime rounds an exact tie down instead (except
    // in rare cases decided by its extended-precision rounding error, which
    // only show in outputs of 17 or more significant digits).
    let (digits, exp) = if is_exact_tie_after_17(v.abs()) {
        let (wide, exp) = sci_digits(v.abs(), 18);
        (wide[..17].to_vec(), exp)
    } else {
        sci_digits(v.abs(), 17)
    };
    Digits { digits, decpt: exp + 1, negative, special: false }
}

/// `n` significant digits of a positive finite value, correctly rounded,
/// and its decimal exponent.
fn sci_digits(v: f64, n: usize) -> (Vec<u8>, i32) {
    let s = format!("{:.*e}", n - 1, v);
    let (mantissa, exp) = s.split_once('e').unwrap();
    (mantissa.bytes().filter(u8::is_ascii_digit).collect(), exp.parse().unwrap())
}

/// Whether the exact decimal value has the digit 5 in 18th place followed by
/// nothing: a tie between two 17-digit roundings.
fn is_exact_tie_after_17(v: f64) -> bool {
    // Rounded to 18 digits, an exact tie still ends in 5; anything else is
    // rejected without the (much slower) long expansions below.
    let (digits, _) = sci_digits(v, 18);
    if digits[17] != b'5' {
        return false;
    }
    // A double's exact decimal expansion has at most 767 significant digits.
    let (digits, _) = sci_digits(v, 40);
    if digits[17] != b'5' || digits[18..].iter().any(|&c| c != b'0') {
        return false;
    }
    let (exact, _) = sci_digits(v, 780);
    exact[17] == b'5' && exact[18..].iter().all(|&c| c == b'0')
}

/// The NaN 0/0 produces on x86/x64 (sign set, quiet, zero payload).
fn is_default_nan(v: f64) -> bool {
    v.to_bits() == 0xfff8_0000_0000_0000
}

/// Rounds the digit string to `n` significant digits, half-up on the next
/// digit (the runtime's rule), and pads with zeros. Returns the digits and
/// the decimal point position after a possible carry.
fn round_to(d: &Digits, n: i32) -> (Vec<u8>, i32) {
    let mut decpt = d.decpt;
    if n < 0 {
        return (Vec::new(), decpt);
    }
    let n = n as usize;
    let mut out: Vec<u8> = d.digits.iter().copied().take(n).collect();
    let next = d.digits.get(n).copied().unwrap_or(b'0');
    if next >= b'5' && next.is_ascii_digit() || (d.special && next >= b'5') {
        // Increment with carry. Non-digit characters ('#', letters) are
        // incremented like digits, which is what turns "1.#I" + 'N' into
        // "1.#J" in the runtime.
        let mut i = out.len();
        loop {
            if i == 0 {
                out.insert(0, b'1');
                decpt += 1;
                out.pop();
                break;
            }
            i -= 1;
            if out[i] == b'9' {
                out[i] = b'0';
            } else {
                out[i] += 1;
                break;
            }
        }
        // Rounding "0" digits up from nothing (n == 0) is handled by callers.
    }
    while out.len() < n {
        out.push(b'0');
    }
    (out, decpt)
}

fn exponent(out: &mut String, conv_upper: bool, e: i32) {
    out.push(if conv_upper { 'E' } else { 'e' });
    out.push(if e < 0 { '-' } else { '+' });
    out.push_str(&format!("{:03}", e.unsigned_abs()));
}

/// `%e`: one digit, point, `precision` digits, exponent.
fn fmt_e(d: &Digits, precision: usize, upper: bool, alt: bool) -> String {
    let (digits, decpt) = round_to(d, precision as i32 + 1);
    let mut s = String::new();
    s.push(digits[0] as char);
    if precision > 0 || alt {
        s.push('.');
    }
    s.extend(digits[1..].iter().map(|&c| c as char));
    let e = if d.special || d.digits.iter().all(|&c| c == b'0') { 0 } else { decpt - 1 };
    exponent(&mut s, upper, e);
    s
}

/// `%f`: all integer digits, point, `precision` digits.
fn fmt_f(d: &Digits, precision: usize, alt: bool) -> String {
    if d.special {
        // Same shape as %e without an exponent: "1.#INF" rounded to the
        // precision.
        let (digits, _) = round_to(d, precision as i32 + 1);
        let mut s = String::new();
        s.push(digits[0] as char);
        if precision > 0 || alt {
            s.push('.');
        }
        s.extend(digits[1..].iter().map(|&c| c as char));
        return s;
    }
    let zero = d.digits.iter().all(|&c| c == b'0');
    let (digits, decpt) = if zero {
        (vec![b'0'; precision], 0)
    } else {
        let n = d.decpt + precision as i32;
        if n < 0 {
            // Rounds to nothing: every printed digit is zero.
            (Vec::new(), d.decpt)
        } else if n == 0 {
            // Only the first digit decides whether it rounds up to 10^-precision.
            if d.digits[0] >= b'5' {
                (vec![b'1'], d.decpt + 1)
            } else {
                (Vec::new(), d.decpt)
            }
        } else {
            round_to(d, n)
        }
    };
    let digit_at = |pos: i32| -> char {
        // pos: index into the full digit sequence where 0 is the first
        // significant digit; positions before it are leading zeros.
        if pos < 0 || pos as usize >= digits.len() {
            '0'
        } else {
            digits[pos as usize] as char
        }
    };
    let mut s = String::new();
    if decpt <= 0 || zero {
        s.push('0');
    } else {
        for i in 0..decpt {
            s.push(digit_at(i));
        }
    }
    if precision > 0 || alt {
        s.push('.');
    }
    for i in 0..precision as i32 {
        let pos = if zero { -1 } else { decpt + i };
        s.push(digit_at(pos));
    }
    s
}

/// `%g`: `precision` significant digits, %e style when the exponent is
/// below -4 or at least the precision; trailing zeros removed unless `#`.
fn fmt_g(d: &Digits, precision: usize, upper: bool, alt: bool) -> String {
    let p = precision.max(1);
    if d.special {
        // Rounded like digits; the zero padding is stripped as for numbers.
        let (mut digits, _) = round_to(d, p as i32);
        if !alt {
            while digits.len() > 1 && digits.last() == Some(&b'0') {
                digits.pop();
            }
        }
        let mut s = String::new();
        s.push(digits[0] as char);
        if digits.len() > 1 || alt {
            s.push('.');
        }
        s.extend(digits[1..].iter().map(|&c| c as char));
        return s;
    }
    let (_, decpt) = round_to(d, p as i32);
    let e = decpt - 1;
    let mut s = if e < -4 || e >= p as i32 {
        fmt_e(d, p - 1, upper, alt)
    } else {
        fmt_f(d, (p as i32 - 1 - e) as usize, alt)
    };
    if !alt {
        // Strip trailing zeros of the fraction (before any exponent).
        let (body, exp) = match s.find(['e', 'E']) {
            Some(i) => (s[..i].to_string(), s[i..].to_string()),
            None => (s.clone(), String::new()),
        };
        let body = if body.contains('.') { body.trim_end_matches('0').trim_end_matches('.').to_string() } else { body };
        s = body + &exp;
    }
    s
}

/// Formats `v` with one conversion specification, MSVCR90 style.
pub fn format(spec: &Spec, v: f64) -> String {
    let d = digits17(v);
    let precision = spec.precision.unwrap_or(6);
    let body = match spec.conv {
        b'e' | b'E' => fmt_e(&d, precision, spec.conv == b'E', spec.alt),
        b'f' => fmt_f(&d, precision, spec.alt),
        _ => fmt_g(&d, precision, spec.conv == b'G', spec.alt),
    };
    let sign = if d.negative {
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
    } else if spec.zero {
        format!("{sign}{}{body}", "0".repeat(pad))
    } else {
        format!("{}{sign}{body}", " ".repeat(pad))
    }
}

/// C entry point used by the Lua build's `sprintf` (src/c/crt.c): formats
/// one double conversion into `out` (capacity `cap`, NUL-terminated).
/// Returns the length written, or -1 if `spec` is not a double conversion.
///
/// # Safety
/// `spec` must be NUL-terminated and `out` valid for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn ds_fmt_double(out: *mut c_char, cap: usize, spec: *const c_char, v: f64) -> c_int {
    let Some(spec) = Spec::parse(CStr::from_ptr(spec).to_bytes()) else { return -1 };
    let s = format(&spec, v);
    let n = s.len().min(cap.saturating_sub(1));
    std::ptr::copy_nonoverlapping(s.as_ptr(), out as *mut u8, n);
    *out.add(n) = 0;
    n as c_int
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(spec: &str, v: f64) -> String {
        format(&Spec::parse(spec.as_bytes()).unwrap(), v)
    }

    #[test]
    fn three_digit_exponents() {
        assert_eq!(f("%.14g", 9.1844232182219e-05), "9.1844232182219e-005");
        assert_eq!(f("%.14g", 1e20), "1e+020");
        assert_eq!(f("%e", 0.0), "0.000000e+000");
    }

    #[test]
    fn plain_values() {
        assert_eq!(f("%.14g", 0.1), "0.1");
        assert_eq!(f("%.14g", 100.0), "100");
        assert_eq!(f("%.14g", -2.5), "-2.5");
        assert_eq!(f("%.2f", 3.14159), "3.14");
        assert_eq!(f("%5.1f", 2.25), "  2.3");
        assert_eq!(f("%.0f", 0.5), "1");
        assert_eq!(f("%.14g", 0.0), "0");
    }

    #[test]
    fn special_values() {
        assert_eq!(f("%.14g", f64::INFINITY), "1.#INF");
        assert_eq!(f("%.14g", f64::NEG_INFINITY), "-1.#INF");
        assert_eq!(f("%.14g", f64::from_bits(0xfff8_0000_0000_0000)), "-1.#IND");
    }
}

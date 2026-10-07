//! JavaScript string semantics (SPEC 3.1, 3.2, 3.5).
//!
//! Strings are held as WTF-8 bytes, so a lone surrogate parsed from a `\uD800` escape survives
//! every slice, trim, and regex replacement and comes back out as the same escape. Lengths and
//! offsets are counted in UTF-16 code units, as JavaScript counts them.

use regex::bytes::Regex;
use std::fmt;
use std::sync::LazyLock;

/// A JavaScript string: WTF-8 bytes (UTF-8 that may also encode lone surrogates).
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct JsStr(Vec<u8>);

impl fmt::Debug for JsStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_string_lossy())
    }
}

impl fmt::Display for JsStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

impl From<&str> for JsStr {
    fn from(s: &str) -> Self {
        JsStr(s.as_bytes().to_vec())
    }
}

impl From<String> for JsStr {
    fn from(s: String) -> Self {
        JsStr(s.into_bytes())
    }
}

impl From<&String> for JsStr {
    fn from(s: &String) -> Self {
        JsStr(s.as_bytes().to_vec())
    }
}

impl PartialEq<str> for JsStr {
    fn eq(&self, other: &str) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<&str> for JsStr {
    fn eq(&self, other: &&str) -> bool {
        self.0 == other.as_bytes()
    }
}

/// Decodes one WTF-8 code point at `i`: (code point, byte length). Invalid bytes, which a
/// well-formed `JsStr` never holds, decode as U+FFFD of length 1.
fn decode_at(b: &[u8], i: usize) -> (u32, usize) {
    let c = b[i];
    if c < 0x80 {
        return (u32::from(c), 1);
    }
    let (len, init) = if c >= 0xF0 {
        (4, u32::from(c & 0x07))
    } else if c >= 0xE0 {
        (3, u32::from(c & 0x0F))
    } else if c >= 0xC0 {
        (2, u32::from(c & 0x1F))
    } else {
        return (0xFFFD, 1);
    };
    if i + len > b.len() {
        return (0xFFFD, 1);
    }
    let mut cp = init;
    for k in 1..len {
        let cc = b[i + k];
        if cc & 0xC0 != 0x80 {
            return (0xFFFD, 1);
        }
        cp = (cp << 6) | u32::from(cc & 0x3F);
    }
    (cp, len)
}

fn encode_cp(cp: u32, out: &mut Vec<u8>) {
    if cp < 0x80 {
        out.push(cp as u8);
    } else if cp < 0x800 {
        out.push(0xC0 | (cp >> 6) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    } else if cp < 0x10000 {
        out.push(0xE0 | (cp >> 12) as u8);
        out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    } else {
        out.push(0xF0 | (cp >> 18) as u8);
        out.push(0x80 | ((cp >> 12) & 0x3F) as u8);
        out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    }
}

impl JsStr {
    /// An empty string.
    pub fn new() -> Self {
        JsStr(Vec::new())
    }

    /// Wraps bytes already known to be WTF-8.
    pub fn from_wtf8(bytes: Vec<u8>) -> Self {
        JsStr(bytes)
    }

    /// Builds a string from UTF-16 code units, pairing surrogates where they pair.
    pub fn from_utf16(units: &[u16]) -> Self {
        let mut out = Vec::with_capacity(units.len());
        let mut i = 0;
        while i < units.len() {
            let u = u32::from(units[i]);
            if (0xD800..0xDC00).contains(&u) && i + 1 < units.len() {
                let v = u32::from(units[i + 1]);
                if (0xDC00..0xE000).contains(&v) {
                    encode_cp(0x10000 + ((u - 0xD800) << 10) + (v - 0xDC00), &mut out);
                    i += 2;
                    continue;
                }
            }
            encode_cp(u, &mut out);
            i += 1;
        }
        JsStr(out)
    }

    /// Appends one code point (a surrogate value appends a lone surrogate).
    pub fn push_code_point(&mut self, cp: u32) {
        // A high surrogate followed by a low one must become one 4-byte sequence, as WTF-8
        // concatenation requires.
        if (0xDC00..0xE000).contains(&cp) && self.0.len() >= 3 {
            let n = self.0.len();
            let (prev, len) = decode_at(&self.0, n - 3);
            if len == 3 && (0xD800..0xDC00).contains(&prev) {
                self.0.truncate(n - 3);
                encode_cp(0x10000 + ((prev - 0xD800) << 10) + (cp - 0xDC00), &mut self.0);
                return;
            }
        }
        encode_cp(cp, &mut self.0);
    }

    /// Appends `other` (JavaScript `+`): a lone high surrogate at the end of this string and a
    /// lone low surrogate at the start of `other` join into one code point, as WTF-8 requires.
    pub fn push_js(&mut self, other: &JsStr) {
        let mut cps = other.code_points();
        if let Some(first) = cps.next() {
            self.push_code_point(first);
            let rest_start = if first >= 0x10000 {
                4
            } else {
                let mut tmp = Vec::new();
                encode_cp(first, &mut tmp);
                tmp.len()
            };
            self.0.extend_from_slice(&other.0[rest_start..]);
        }
    }

    /// Appends a Rust string.
    pub fn push_str(&mut self, s: &str) {
        self.push_js(&JsStr::from(s));
    }

    /// The WTF-8 bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The WTF-8 bytes, by value.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Whether the string is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The text as `&str` when it holds no lone surrogate.
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// UTF-8 with every lone surrogate replaced by U+FFFD (for hashing and terminals, 3.3).
    pub fn to_string_lossy(&self) -> String {
        if let Ok(s) = std::str::from_utf8(&self.0) {
            s.to_string()
        } else {
            let mut out = String::with_capacity(self.0.len());
            for cp in self.code_points() {
                out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
            }
            out
        }
    }

    /// The code points, lone surrogates included.
    pub fn code_points(&self) -> impl Iterator<Item = u32> + '_ {
        let b = &self.0;
        let mut i = 0;
        std::iter::from_fn(move || {
            if i >= b.len() {
                return None;
            }
            let (cp, len) = decode_at(b, i);
            i += len;
            Some(cp)
        })
    }

    /// The UTF-16 code units, as JavaScript sees the string.
    pub fn utf16(&self) -> Vec<u16> {
        let mut out = Vec::with_capacity(self.0.len());
        for cp in self.code_points() {
            if cp >= 0x10000 {
                let v = cp - 0x10000;
                out.push((0xD800 + (v >> 10)) as u16);
                out.push((0xDC00 + (v & 0x3FF)) as u16);
            } else {
                out.push(cp as u16);
            }
        }
        out
    }

    /// `length`: UTF-16 code units.
    pub fn utf16_len(&self) -> usize {
        utf16_len_bytes(&self.0)
    }

    /// `slice(0, n)` in UTF-16 units. A cut through a surrogate pair drops the pair (3.2).
    pub fn slice_units(&self, n: usize) -> JsStr {
        let b = &self.0;
        let mut i = 0;
        let mut units = 0;
        while i < b.len() {
            let (cp, len) = decode_at(b, i);
            let w = if cp >= 0x10000 { 2 } else { 1 };
            if units + w > n {
                break;
            }
            units += w;
            i += len;
        }
        JsStr(b[..i].to_vec())
    }

    /// `padEnd(n)` with spaces, in UTF-16 units.
    pub fn pad_end(&self, n: usize) -> JsStr {
        let len = self.utf16_len();
        let mut out = self.clone();
        for _ in len..n {
            out.0.push(b' ');
        }
        out
    }

    /// `String.prototype.trim()`: strips exactly the JSWS characters (3.5).
    pub fn trim(&self) -> JsStr {
        JsStr(trim_bytes(&self.0).to_vec())
    }

    /// `trimEnd()`.
    pub fn trim_end(&self) -> JsStr {
        let b = &self.0;
        let mut end = b.len();
        while end > 0 {
            let start = prev_start(b, end);
            let (cp, _) = decode_at(b, start);
            if !is_jsws(cp) {
                break;
            }
            end = start;
        }
        JsStr(b[..end].to_vec())
    }

    /// `String.prototype.includes` for a Rust string needle.
    pub fn contains(&self, needle: &str) -> bool {
        find_bytes(&self.0, needle.as_bytes()).is_some()
    }

    /// `toLowerCase()` restricted to what the callers need: ASCII letters (settings values that
    /// must match an ASCII word anyway compare equal either way).
    pub fn to_ascii_lowercase(&self) -> JsStr {
        JsStr(self.0.to_ascii_lowercase())
    }

    /// `toUpperCase()` (full Unicode mapping for valid text; lone surrogates are kept).
    pub fn to_upper(&self) -> JsStr {
        let mut out = JsStr::new();
        for cp in self.code_points() {
            match char::from_u32(cp) {
                Some(c) => {
                    for u in c.to_uppercase() {
                        out.push_code_point(u as u32);
                    }
                }
                None => out.push_code_point(cp),
            }
        }
        out
    }

    /// Replaces every match of `re` with `with`.
    pub fn replace_all(&self, re: &Regex, with: &str) -> JsStr {
        JsStr(re.replace_all(&self.0, with.as_bytes()).into_owned())
    }

    /// Splits on a literal byte string (as `split(sep)` with a string separator).
    pub fn split_on(&self, sep: &str) -> Vec<JsStr> {
        let s = sep.as_bytes();
        let mut out = Vec::new();
        let mut rest: &[u8] = &self.0;
        while let Some(i) = find_bytes(rest, s) {
            out.push(JsStr(rest[..i].to_vec()));
            rest = &rest[i + s.len()..];
        }
        out.push(JsStr(rest.to_vec()));
        out
    }

    /// Compares by UTF-16 code units, as `<` on JavaScript strings does.
    pub fn cmp_utf16(&self, other: &JsStr) -> std::cmp::Ordering {
        self.utf16().cmp(&other.utf16())
    }
}

fn prev_start(b: &[u8], end: usize) -> usize {
    let mut s = end - 1;
    while s > 0 && b[s] & 0xC0 == 0x80 && end - s < 4 {
        s -= 1;
    }
    s
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// UTF-16 length of WTF-8 bytes: a 4-byte sequence is two units, everything else one.
pub fn utf16_len_bytes(b: &[u8]) -> usize {
    let mut n = 0;
    for &c in b {
        if c & 0xC0 != 0x80 {
            n += if c >= 0xF0 { 2 } else { 1 };
        }
    }
    n
}

/// The JavaScript whitespace set `JSWS` (3.1).
pub fn is_jsws(cp: u32) -> bool {
    matches!(
        cp,
        0x09..=0x0D
            | 0x20
            | 0xA0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028
            | 0x2029
            | 0x202F
            | 0x205F
            | 0x3000
            | 0xFEFF
    )
}

/// Trims JSWS from both ends of WTF-8 bytes.
pub fn trim_bytes(b: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < b.len() {
        let (cp, len) = decode_at(b, start);
        if !is_jsws(cp) {
            break;
        }
        start += len;
    }
    let mut end = b.len();
    while end > start {
        let s = prev_start(b, end);
        let (cp, _) = decode_at(b, s);
        if !is_jsws(cp) {
            break;
        }
        end = s;
    }
    &b[start..end]
}

/// `String.prototype.trim()` for a Rust string.
pub fn trim_str(s: &str) -> &str {
    let b = trim_bytes(s.as_bytes());
    // Trimming JSWS only removes whole code points, so the slice stays valid UTF-8.
    std::str::from_utf8(b).unwrap_or("")
}

/// The JSWS class written out for a regex in Unicode mode (3.1).
pub const JSWS: &str =
    r"[\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";

/// `<system-reminder>[\s\S]*?</system-reminder>`, crossing any byte including surrogates.
pub static SYSTEM_REMINDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<system-reminder>(?s-u:.)*?</system-reminder>").unwrap());

/// `\s+` with the JavaScript whitespace set.
pub static JSWS_RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("{JSWS}+")).unwrap());

// ---------------------------------------------------------------------------------------------
// Numbers (3.6, 3.7)

/// `Math.round(x)` (3.7).
pub fn math_round(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let r = x.floor();
    let out = if x - r >= 0.5 { r + 1.0 } else { r };
    if out == 0.0 && x < 0.0 { -0.0 } else { out }
}

/// `x.toFixed(2)` (3.7).
pub fn to_fixed2(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.abs() >= 1e21 {
        return number_to_string(x);
    }
    if x < 0.0 {
        let inner = to_fixed2(-x);
        // `-0` prints `0.00`, and a negative that rounds to zero keeps its sign as JS does.
        return format!("-{inner}");
    }
    if x == 0.0 {
        return "0.00".to_string();
    }
    // Exact decimal value of the double: mantissa * 2^exp.
    let bits = x.to_bits();
    let exp_bits = ((bits >> 52) & 0x7FF) as i64;
    let frac = bits & ((1u64 << 52) - 1);
    let (mant, exp) = if exp_bits == 0 { (frac, -1074i64) } else { (frac | (1u64 << 52), exp_bits - 1075) };
    // n = round(x * 100), ties to the larger n. x*100 = mant * 100 * 2^exp.
    let n: u128 = if exp >= 0 {
        u128::from(mant) * 100 * (1u128 << exp.min(70))
    } else {
        let shift = (-exp) as u32;
        if shift >= 120 {
            0
        } else {
            let num = u128::from(mant) * 100;
            let q = num >> shift;
            let rem = num - (q << shift);
            let half = 1u128 << (shift - 1);
            if rem >= half { q + 1 } else { q }
        }
    };
    format!("{}.{:02}", n / 100, n % 100)
}

/// `Number.prototype.toString()` (3.3).
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let e = format!("{:e}", x.abs());
    let (mantissa, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp + 1; // position of the decimal point relative to the digits
    let body = if k <= n && n <= 21 {
        format!("{}{}", digits, "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{}", "0".repeat((-n) as usize), digits)
    } else {
        let e = n - 1;
        let es = if e >= 0 { format!("+{e}") } else { format!("{e}") };
        if k == 1 { format!("{digits}e{es}") } else { format!("{}.{}e{}", &digits[..1], &digits[1..], es) }
    };
    format!("{sign}{body}")
}

/// ECMAScript `StringToNumber` (3.6).
pub fn string_to_number(s: &JsStr) -> f64 {
    let t = s.trim();
    let Some(t) = t.as_str() else {
        return f64::NAN;
    };
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(rest) = t.strip_prefix(prefix) {
            if rest.is_empty() || !rest.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            let mut v = 0f64;
            for c in rest.chars() {
                v = v * f64::from(radix) + f64::from(c.to_digit(radix).unwrap());
            }
            return v;
        }
    }
    if is_decimal_literal(t) { t.parse::<f64>().unwrap_or(f64::NAN) } else { f64::NAN }
}

/// `StrDecimalLiteral`: optional sign, digits with optional fraction, optional exponent.
fn is_decimal_literal(t: &str) -> bool {
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let s = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - s;
    }
    if int_digits == 0 && frac_digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let s = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == s {
            return false;
        }
    }
    i == b.len()
}

// ---------------------------------------------------------------------------------------------
// Regex helpers for patterns that use lookahead in Node (3.1)

/// Searches `re` (a pattern whose JavaScript form ended in `(?![-\w])`) from the start of
/// `text`, restarting at a failed match's start + 1, and returns the first passing match.
pub fn find_without_word_after(re: &Regex, text: &[u8]) -> Option<(usize, usize)> {
    let mut at = 0;
    while at <= text.len() {
        let m = re.find_at(text, at)?;
        let ok = match text.get(m.end()) {
            None => true,
            Some(&c) => !(c == b'-' || c == b'_' || c.is_ascii_alphanumeric()),
        };
        if ok {
            return Some((m.start(), m.end()));
        }
        at = m.start() + 1;
    }
    None
}

/// The optional `-(\d{1,2})(?!\d)` minor version after a major that ends at `end`: the digit
/// run after a `-` when it is one or two digits long, else none.
pub fn minor_after(text: &[u8], end: usize) -> Option<&[u8]> {
    if text.get(end) != Some(&b'-') {
        return None;
    }
    let start = end + 1;
    let mut i = start;
    while i < text.len() && text[i].is_ascii_digit() {
        i += 1;
    }
    let len = i - start;
    if (1..=2).contains(&len) { Some(&text[start..i]) } else { None }
}

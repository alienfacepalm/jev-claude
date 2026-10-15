//! JSON as JavaScript's `JSON.parse` and `JSON.stringify` see it, for the harness alone.
//!
//! The harness compares bodies byte for byte and hashes the text of its inputs, so it needs
//! JavaScript's serialisation exactly: object keys in insertion order with integer-like keys first
//! (ECMAScript `OrdinaryOwnPropertyKeys`), shortest round-trip numbers written as `Number#toString`
//! writes them, and `JSON.stringify`'s string escapes. It is written here rather than taken from
//! the crate under test (`jev_router::jsjson`), so the harness shares no code with the programs it
//! checks. The inputs are well-formed UTF-8 (no lone surrogates), which is all it supports.

use std::fmt::Write as _;

/// A JSON value. Objects keep their keys in JavaScript's order.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl From<&str> for Json {
    fn from(s: &str) -> Json {
        Json::Str(s.to_string())
    }
}

impl From<String> for Json {
    fn from(s: String) -> Json {
        Json::Str(s)
    }
}

impl From<f64> for Json {
    fn from(n: f64) -> Json {
        Json::Num(n)
    }
}

impl From<bool> for Json {
    fn from(b: bool) -> Json {
        Json::Bool(b)
    }
}

impl From<Vec<Json>> for Json {
    fn from(items: Vec<Json>) -> Json {
        Json::Arr(items)
    }
}

/// An object built key by key, as an object literal would be.
pub fn obj(pairs: Vec<(&str, Json)>) -> Json {
    let mut out = Json::Obj(Vec::new());
    for (key, value) in pairs {
        out.set(key, value);
    }
    out
}

/// The array index a key names (a canonical decimal below 2^32 - 1), which JavaScript orders first.
fn array_index(key: &str) -> Option<u64> {
    let canonical = !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) && (key == "0" || !key.starts_with('0'));
    if !canonical || key.len() > 10 {
        return None;
    }
    key.parse::<u64>().ok().filter(|&n| n < 4_294_967_295)
}

impl Json {
    /// The value under `key` of an object, if there is one.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The value under `key`, which must exist.
    pub fn at(&self, key: &str) -> &Json {
        self.get(key).unwrap_or_else(|| panic!("no key {key:?} in {}", stringify(self)))
    }

    /// The mutable value under `key`, which must exist.
    pub fn at_mut(&mut self, key: &str) -> &mut Json {
        let text = stringify(self);
        match self {
            Json::Obj(pairs) => {
                pairs.iter_mut().find(|(k, _)| k == key).map_or_else(|| panic!("no key {key:?} in {text}"), |(_, v)| v)
            }
            _ => panic!("not an object: {text}"),
        }
    }

    /// Item `i` of an array, which must exist.
    pub fn idx(&self, i: usize) -> &Json {
        self.arr().get(i).unwrap_or_else(|| panic!("no item {i} in {}", stringify(self)))
    }

    /// The items of an array, which this must be.
    pub fn arr(&self) -> &Vec<Json> {
        match self {
            Json::Arr(items) => items,
            other => panic!("not an array: {}", stringify(other)),
        }
    }

    /// The mutable items of an array, which this must be.
    pub fn arr_mut(&mut self) -> &mut Vec<Json> {
        match self {
            Json::Arr(items) => items,
            other => panic!("not an array: {}", stringify(other)),
        }
    }

    /// The keys of an object, in order (none for anything else).
    pub fn keys(&self) -> Vec<String> {
        match self {
            Json::Obj(pairs) => pairs.iter().map(|(k, _)| k.clone()).collect(),
            _ => Vec::new(),
        }
    }

    /// The text of a string, or `None`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The text of a string, which this must be.
    pub fn str(&self) -> &str {
        self.as_str().unwrap_or_else(|| panic!("not a string: {}", stringify(self)))
    }

    /// The number, or `None`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// `obj[key] = value`: replaces in place, or adds the key where JavaScript would put it.
    pub fn set(&mut self, key: &str, value: Json) {
        let Json::Obj(pairs) = self else { panic!("set {key:?} on a non-object") };
        if let Some(slot) = pairs.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value;
            return;
        }
        let at = match array_index(key) {
            Some(n) => pairs.iter().position(|(k, _)| array_index(k).is_none_or(|m| m > n)).unwrap_or(pairs.len()),
            None => pairs.len(),
        };
        pairs.insert(at, (key.to_string(), value));
    }

    /// `{...self, key: value, ...}`: a copy with each pair set in turn.
    pub fn with(&self, pairs: Vec<(&str, Json)>) -> Json {
        let mut out = self.clone();
        for (key, value) in pairs {
            out.set(key, value);
        }
        out
    }

    /// `delete obj[key]`.
    pub fn remove(&mut self, key: &str) -> Option<Json> {
        let Json::Obj(pairs) = self else { return None };
        let at = pairs.iter().position(|(k, _)| k == key)?;
        Some(pairs.remove(at).1)
    }
}

// -------------------------------------------------------------------------------------------------
// JSON.stringify

/// `JSON.stringify(value)` with no indentation.
pub fn stringify(value: &Json) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Json) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => out.push_str(&number(*n)),
        Json::Str(s) => quote(out, s),
        Json::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Json::Obj(pairs) => {
            out.push('{');
            for (i, (key, item)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                quote(out, key);
                out.push(':');
                write_value(out, item);
            }
            out.push('}');
        }
    }
}

/// `JSON.stringify`'s `QuoteJSONString` for well-formed text.
fn quote(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `Number#toString()` (ECMAScript `Number::toString`), with `JSON.stringify`'s `null` for NaN
/// and the infinities. Rust's `{:e}` gives the same shortest round-trip digits; only the layout
/// differs, and the layout rules are the specification's.
pub fn number(n: f64) -> String {
    if !n.is_finite() {
        return "null".into();
    }
    if n == 0.0 {
        return "0".into();
    }
    let scientific = format!("{:e}", n.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("{:e} has an exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i64;
    // The value is 0.<digits> x 10^point.
    let point = exponent.parse::<i64>().expect("integer exponent") + 1;
    let body = if k <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let (first, rest) = digits.split_at(1);
        let e = point - 1;
        let dot = if rest.is_empty() { "" } else { "." };
        format!("{first}{dot}{rest}e{}{}", if e < 0 { "-" } else { "+" }, e.abs())
    };
    if n < 0.0 { format!("-{body}") } else { body }
}

// -------------------------------------------------------------------------------------------------
// JSON.parse

/// `JSON.parse(text)`; an error names the byte offset.
pub fn parse(text: &str) -> Result<Json, String> {
    let mut p = Parser { b: text.as_bytes(), i: 0 };
    p.ws();
    let value = p.value()?;
    p.ws();
    if p.i != p.b.len() {
        return Err(p.error("unexpected text after the value"));
    }
    Ok(value)
}

/// `JSON.parse` of a file's bytes, which must be UTF-8 JSON.
pub fn parse_bytes(bytes: &[u8]) -> Result<Json, String> {
    parse(std::str::from_utf8(bytes).map_err(|e| format!("not UTF-8: {e}"))?)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn error(&self, what: &str) -> String {
        format!("JSON: {what} at byte {}", self.i)
    }

    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn eat(&mut self, literal: &str) -> bool {
        if self.b[self.i..].starts_with(literal.as_bytes()) {
            self.i += literal.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        match self.b.get(self.i) {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ if self.eat("null") => Ok(Json::Null),
            _ if self.eat("true") => Ok(Json::Bool(true)),
            _ if self.eat("false") => Ok(Json::Bool(false)),
            _ => Err(self.error("unexpected token")),
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.i += 1;
        let mut out = Json::Obj(Vec::new());
        self.ws();
        if self.eat("}") {
            return Ok(out);
        }
        loop {
            self.ws();
            if self.b.get(self.i) != Some(&b'"') {
                return Err(self.error("expected a key"));
            }
            let key = self.string()?;
            self.ws();
            if !self.eat(":") {
                return Err(self.error("expected ':'"));
            }
            self.ws();
            let value = self.value()?;
            // A repeated key keeps its first position and takes the last value, as JSON.parse does.
            out.set(&key, value);
            self.ws();
            if self.eat("}") {
                return Ok(out);
            }
            if !self.eat(",") {
                return Err(self.error("expected ',' or '}'"));
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.eat("]") {
            return Ok(Json::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            if self.eat("]") {
                return Ok(Json::Arr(items));
            }
            if !self.eat(",") {
                return Err(self.error("expected ',' or ']'"));
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self.b.get(self.i..self.i + 4).ok_or_else(|| self.error("short \\u escape"))?;
        let text = std::str::from_utf8(digits).map_err(|_| self.error("bad \\u escape"))?;
        let unit = u32::from_str_radix(text, 16).map_err(|_| self.error("bad \\u escape"))?;
        self.i += 4;
        Ok(unit)
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out = String::new();
        let mut start = self.i;
        loop {
            let Some(&byte) = self.b.get(self.i) else { return Err(self.error("unterminated string")) };
            match byte {
                b'"' => {
                    out.push_str(std::str::from_utf8(&self.b[start..self.i]).expect("input is UTF-8"));
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(std::str::from_utf8(&self.b[start..self.i]).expect("input is UTF-8"));
                    self.i += 1;
                    let escape = *self.b.get(self.i).ok_or_else(|| self.error("unterminated escape"))?;
                    self.i += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let unit = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&unit) && self.eat("\\u") {
                                let low = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&low) {
                                    return Err(self.error("lone surrogate (unsupported here)"));
                                }
                                0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
                            } else {
                                unit
                            };
                            out.push(
                                char::from_u32(code).ok_or_else(|| self.error("lone surrogate (unsupported here)"))?,
                            );
                        }
                        _ => return Err(self.error("bad escape")),
                    }
                    start = self.i;
                }
                0..0x20 => return Err(self.error("control character in a string")),
                _ => self.i += 1,
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        let digits = |p: &mut Parser| {
            let from = p.i;
            while p.b.get(p.i).is_some_and(u8::is_ascii_digit) {
                p.i += 1;
            }
            p.i > from
        };
        self.eat("-");
        if !self.eat("0") && !digits(self) {
            return Err(self.error("bad number"));
        }
        if self.eat(".") && !digits(self) {
            return Err(self.error("bad fraction"));
        }
        if matches!(self.b.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if !self.eat("+") {
                self.eat("-");
            }
            if !digits(self) {
                return Err(self.error("bad exponent"));
            }
        }
        let text = std::str::from_utf8(&self.b[start..self.i]).expect("ASCII");
        text.parse::<f64>().map(Json::Num).map_err(|_| self.error("bad number"))
    }
}

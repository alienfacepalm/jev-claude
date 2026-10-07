//! JSON with JavaScript semantics (SPEC 3.3): `JSON.parse` and `JSON.stringify` as Node runs
//! them, including object key order (array-index keys first), IEEE-754 numbers printed the way
//! `Number.prototype.toString` prints them, lone surrogates that round-trip, and `undefined`.

use crate::jsstr::{JsStr, math_round, number_to_string, string_to_number};

/// A JavaScript value as JSON can produce or carry it.
#[derive(Clone, Debug, Default)]
pub enum Value {
    /// `undefined`: omitted from objects and written as `null` in arrays by `stringify`.
    #[default]
    Undefined,
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A number (an IEEE-754 double, as in JavaScript).
    Number(f64),
    /// A string, which may hold lone surrogates.
    String(JsStr),
    /// An array.
    Array(Vec<Value>),
    /// An object, keys in JavaScript order.
    Object(Object),
}

/// An object whose keys iterate in JavaScript order: canonical array-index keys ascending, then
/// every other key in insertion order.
#[derive(Clone, Debug, Default)]
pub struct Object {
    entries: Vec<(JsStr, Value)>,
}

/// The numeric value of a canonical array-index key, or None.
fn array_index(key: &JsStr) -> Option<u32> {
    let b = key.as_bytes();
    if b.is_empty() || b.len() > 10 || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if b.len() > 1 && b[0] == b'0' {
        return None;
    }
    let v: u64 = std::str::from_utf8(b).ok()?.parse().ok()?;
    if v < 4_294_967_295 { Some(v as u32) } else { None }
}

impl Object {
    /// An empty object.
    pub fn new() -> Self {
        Object { entries: Vec::new() }
    }

    /// The number of properties.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the object has no properties.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The value of property `key`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The value of property `key`, for a key that may hold lone surrogates.
    pub fn get_js(&self, key: &JsStr) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// A mutable reference to the value of property `key`.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Whether property `key` exists.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Sets a property: an existing key keeps its position and takes the new value.
    pub fn insert(&mut self, key: impl Into<JsStr>, value: Value) {
        let key = key.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
            return;
        }
        match array_index(&key) {
            Some(idx) => {
                let pos = self
                    .entries
                    .iter()
                    .position(|(k, _)| array_index(k).is_none_or(|other| other > idx))
                    .unwrap_or(self.entries.len());
                self.entries.insert(pos, (key, value));
            }
            None => self.entries.push((key, value)),
        }
    }

    /// `delete obj[key]`.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let pos = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(pos).1)
    }

    /// `delete obj[key]` for a key that may hold lone surrogates.
    pub fn remove_js(&mut self, key: &JsStr) -> Option<Value> {
        let pos = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(pos).1)
    }

    /// Properties in JavaScript order.
    pub fn iter(&self) -> impl Iterator<Item = (&JsStr, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// Properties in JavaScript order, values mutable.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&JsStr, &mut Value)> {
        self.entries.iter_mut().map(|(k, v)| (&*k, v))
    }

    /// Keys in JavaScript order.
    pub fn keys(&self) -> impl Iterator<Item = &JsStr> {
        self.entries.iter().map(|(k, _)| k)
    }

    /// `Object.keys(obj).length`: properties whose value is not `undefined` still count, as
    /// JavaScript counts every own property.
    pub fn key_count(&self) -> usize {
        self.entries.len()
    }

    /// `{...this, ...other}`.
    pub fn spread(&mut self, other: &Object) {
        for (k, v) in &other.entries {
            self.insert(k.clone(), v.clone());
        }
    }
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self.entries.iter().zip(&other.entries).all(|((a, x), (b, y))| a == b && x == y)
    }
}

/// Deep equality as tests compare parsed JSON (`NaN` equals `NaN`; `-0` equals `0`).
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b || (a.is_nan() && b.is_nan()),
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a == b,
            _ => false,
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(JsStr::from(s))
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(JsStr::from(s))
    }
}
impl From<JsStr> for Value {
    fn from(s: JsStr) -> Self {
        Value::String(s)
    }
}
impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}
impl From<Object> for Value {
    fn from(o: Object) -> Self {
        Value::Object(o)
    }
}
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(o: Option<T>) -> Self {
        o.map_or(Value::Null, Into::into)
    }
}

static UNDEFINED: Value = Value::Undefined;

impl Value {
    /// `value?.[key]`: `undefined` for anything that is not an object with that key. Strings
    /// and arrays expose only their indices here, as no caller reads `length` through it.
    pub fn get(&self, key: &str) -> &Value {
        match self {
            Value::Object(o) => o.get(key).unwrap_or(&UNDEFINED),
            Value::Array(a) => key.parse::<usize>().ok().and_then(|i| a.get(i)).unwrap_or(&UNDEFINED),
            _ => &UNDEFINED,
        }
    }

    /// `value[i]` for an array; `undefined` otherwise or out of range.
    pub fn idx(&self, i: usize) -> &Value {
        match self {
            Value::Array(a) => a.get(i).unwrap_or(&UNDEFINED),
            _ => &UNDEFINED,
        }
    }

    /// Whether the value is `null` or `undefined`.
    pub fn is_nullish(&self) -> bool {
        matches!(self, Value::Undefined | Value::Null)
    }

    /// Whether the value is `undefined`.
    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }

    /// `a ?? b`.
    pub fn or<'a>(&'a self, other: &'a Value) -> &'a Value {
        if self.is_nullish() { other } else { self }
    }

    /// JavaScript truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Array(_) | Value::Object(_) => true,
        }
    }

    /// The string, if this is one.
    pub fn as_str(&self) -> Option<&JsStr> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The number, if this is one.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// The elements, if this is an array.
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The object, if this is one.
    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    /// The object, mutably, if this is one.
    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    /// Strict equality with a string.
    pub fn is_str(&self, s: &str) -> bool {
        matches!(self, Value::String(v) if v == s)
    }

    /// `String(value)` / template-literal conversion.
    pub fn to_js_string(&self) -> JsStr {
        match self {
            Value::Undefined => "undefined".into(),
            Value::Null => "null".into(),
            Value::Bool(b) => (if *b { "true" } else { "false" }).into(),
            Value::Number(n) => number_to_string(*n).into(),
            Value::String(s) => s.clone(),
            Value::Array(a) => join_array(a, ","),
            Value::Object(_) => "[object Object]".into(),
        }
    }

    /// ECMAScript `ToNumber` (3.6).
    pub fn to_number(&self) -> f64 {
        match self {
            Value::Undefined | Value::Object(_) => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Number(n) => *n,
            Value::String(s) => string_to_number(s),
            Value::Array(a) => string_to_number(&join_array(a, ",")),
        }
    }

    /// `{...value}`: the own enumerable properties an object spread copies.
    pub fn spread_of(&self) -> Object {
        let mut out = Object::new();
        match self {
            Value::Object(o) => out = o.clone(),
            Value::Array(a) => {
                for (i, v) in a.iter().enumerate() {
                    out.insert(i.to_string(), v.clone());
                }
            }
            Value::String(s) => {
                for (i, u) in s.utf16().iter().enumerate() {
                    out.insert(i.to_string(), Value::String(JsStr::from_utf16(&[*u])));
                }
            }
            _ => {}
        }
        out
    }

    /// `Object.keys(value).length` (primitives other than strings have none).
    pub fn own_key_count(&self) -> usize {
        match self {
            Value::Object(o) => o.key_count(),
            Value::Array(a) => a.len(),
            Value::String(s) => s.utf16_len(),
            _ => 0,
        }
    }
}

/// `Array.prototype.join(sep)`: `null` and `undefined` elements are empty.
pub fn join_array(a: &[Value], sep: &str) -> JsStr {
    let mut out = JsStr::new();
    for (i, v) in a.iter().enumerate() {
        if i > 0 {
            out.push_str(sep);
        }
        if !v.is_nullish() {
            out.push_js(&v.to_js_string());
        }
    }
    out
}

/// `Math.round(ToNumber(v))`.
pub fn round_value(v: &Value) -> f64 {
    math_round(v.to_number())
}

// ---------------------------------------------------------------------------------------------
// Parse

/// `JSON.parse(buffer.toString())`: invalid UTF-8 becomes U+FFFD, then strict JSON.
pub fn parse_bytes(bytes: &[u8]) -> Result<Value, String> {
    let text = String::from_utf8_lossy(bytes);
    parse(&text)
}

/// `JSON.parse(text)`.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser { s: text.as_bytes(), i: 0 };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("Unexpected non-whitespace character after JSON"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self, what: &str) -> String {
        format!("{what} at position {}", self.i)
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > 5000 {
            return Err(self.err("Maximum nesting depth exceeded"));
        }
        match self.s.get(self.i) {
            None => Err(self.err("Unexpected end of JSON input")),
            Some(b'{') => {
                self.i += 1;
                let mut obj = Object::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Value::Object(obj));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return Err(self.err("Expected property name"));
                    }
                    let key = self.string()?;
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') {
                        return Err(self.err("Expected ':' after property name"));
                    }
                    self.i += 1;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    obj.insert(key, v);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Object(obj));
                        }
                        _ => return Err(self.err("Expected ',' or '}' after property value")),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut arr = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Value::Array(arr));
                }
                loop {
                    self.ws();
                    arr.push(self.value(depth + 1)?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Array(arr));
                        }
                        _ => return Err(self.err("Expected ',' or ']' after array element")),
                    }
                }
            }
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(c) if *c == b'-' || c.is_ascii_digit() => self.number(),
            Some(_) => Err(self.err("Unexpected token")),
        }
    }

    fn literal(&mut self, word: &str, v: Value) -> Result<Value, String> {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(self.err("Unexpected token"))
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        let s = self.s;
        if s.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        match s.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(c) if c.is_ascii_digit() => {
                while s.get(self.i).is_some_and(u8::is_ascii_digit) {
                    self.i += 1;
                }
            }
            _ => return Err(self.err("No number after minus sign")),
        }
        if s.get(self.i) == Some(&b'.') {
            self.i += 1;
            if !s.get(self.i).is_some_and(u8::is_ascii_digit) {
                return Err(self.err("Unterminated fractional number"));
            }
            while s.get(self.i).is_some_and(u8::is_ascii_digit) {
                self.i += 1;
            }
        }
        if matches!(s.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !s.get(self.i).is_some_and(u8::is_ascii_digit) {
                return Err(self.err("Exponent part is missing a number"));
            }
            while s.get(self.i).is_some_and(u8::is_ascii_digit) {
                self.i += 1;
            }
        }
        let text = std::str::from_utf8(&s[start..self.i]).unwrap();
        text.parse::<f64>().map(Value::Number).map_err(|_| self.err("Bad number"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut v = 0u32;
        for _ in 0..4 {
            let c = *self.s.get(self.i).ok_or_else(|| self.err("Bad Unicode escape"))?;
            let d = (c as char).to_digit(16).ok_or_else(|| self.err("Bad Unicode escape"))?;
            v = v * 16 + d;
            self.i += 1;
        }
        Ok(v)
    }

    fn string(&mut self) -> Result<JsStr, String> {
        self.i += 1; // opening quote
        let mut out = JsStr::new();
        let mut run_start = self.i;
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return Err(self.err("Unterminated string in JSON"));
            };
            match c {
                b'"' => {
                    out.push_str(std::str::from_utf8(&self.s[run_start..self.i]).unwrap());
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(std::str::from_utf8(&self.s[run_start..self.i]).unwrap());
                    self.i += 1;
                    let Some(&e) = self.s.get(self.i) else {
                        return Err(self.err("Bad escaped character"));
                    };
                    self.i += 1;
                    match e {
                        b'"' => out.push_str("\""),
                        b'\\' => out.push_str("\\"),
                        b'/' => out.push_str("/"),
                        b'b' => out.push_str("\u{8}"),
                        b'f' => out.push_str("\u{c}"),
                        b'n' => out.push_str("\n"),
                        b'r' => out.push_str("\r"),
                        b't' => out.push_str("\t"),
                        b'u' => {
                            let cp = self.hex4()?;
                            out.push_code_point(cp);
                        }
                        _ => {
                            self.i -= 1;
                            return Err(self.err("Bad escaped character"));
                        }
                    }
                    run_start = self.i;
                }
                c if c < 0x20 => return Err(self.err("Bad control character in string literal")),
                _ => self.i += 1,
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Stringify

/// `JSON.stringify(value)`; None where JavaScript returns `undefined`.
pub fn stringify(v: &Value) -> Option<JsStr> {
    if v.is_undefined() {
        return None;
    }
    let mut out = Vec::new();
    write_value(v, &mut out, None, 0);
    Some(JsStr::from_wtf8(out))
}

/// `JSON.stringify(value, null, 2)`.
pub fn stringify_pretty(v: &Value) -> Option<JsStr> {
    if v.is_undefined() {
        return None;
    }
    let mut out = Vec::new();
    write_value(v, &mut out, Some(2), 0);
    Some(JsStr::from_wtf8(out))
}

/// `stringify` for a value that is never `undefined`, as bytes.
pub fn to_bytes(v: &Value) -> Vec<u8> {
    stringify(v).map(JsStr::into_bytes).unwrap_or_default()
}

fn newline(out: &mut Vec<u8>, indent: Option<usize>, level: usize) {
    if let Some(n) = indent {
        out.push(b'\n');
        out.extend(std::iter::repeat_n(b' ', n * level));
    }
}

fn write_value(v: &Value, out: &mut Vec<u8>, indent: Option<usize>, level: usize) {
    match v {
        Value::Undefined | Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Number(n) => {
            if n.is_finite() {
                out.extend_from_slice(number_to_string(*n).as_bytes());
            } else {
                out.extend_from_slice(b"null");
            }
        }
        Value::String(s) => write_string(s, out),
        Value::Array(a) => {
            if a.is_empty() {
                out.extend_from_slice(b"[]");
                return;
            }
            out.push(b'[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                newline(out, indent, level + 1);
                write_value(item, out, indent, level + 1);
            }
            newline(out, indent, level);
            out.push(b']');
        }
        Value::Object(o) => {
            let members: Vec<_> = o.iter().filter(|(_, v)| !v.is_undefined()).collect();
            if members.is_empty() {
                out.extend_from_slice(b"{}");
                return;
            }
            out.push(b'{');
            for (i, (k, item)) in members.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                newline(out, indent, level + 1);
                write_string(k, out);
                out.push(b':');
                if indent.is_some() {
                    out.push(b' ');
                }
                write_value(item, out, indent, level + 1);
            }
            newline(out, indent, level);
            out.push(b'}');
        }
    }
}

fn write_string(s: &JsStr, out: &mut Vec<u8>) {
    out.push(b'"');
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0C => out.extend_from_slice(b"\\f"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            c if c < 0x20 => out.extend_from_slice(format!("\\u{c:04x}").as_bytes()),
            // A lone surrogate in WTF-8: ED A0..BF xx.
            0xED if i + 2 < b.len() && b[i + 1] >= 0xA0 => {
                let cp = 0xD000 | (u32::from(b[i + 1] & 0x3F) << 6) | u32::from(b[i + 2] & 0x3F);
                out.extend_from_slice(format!("\\u{cp:04x}").as_bytes());
                i += 3;
                continue;
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out.push(b'"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str) -> String {
        stringify(&parse(text).unwrap()).unwrap().to_string_lossy()
    }

    #[test]
    fn spec_examples() {
        assert_eq!(round_trip(r#"{"b":1,"2":2,"a":3,"1":4,"01":5}"#), r#"{"1":4,"2":2,"b":1,"a":3,"01":5}"#);
        assert_eq!(round_trip("12345678901234567890"), "12345678901234567000");
        assert_eq!(round_trip("1.0"), "1");
        assert_eq!(round_trip("-0"), "0");
        assert!(parse("-0").unwrap().as_number().unwrap().is_sign_negative());
        assert_eq!(round_trip(r#"{"a":1,"a":2,"b":3}"#), r#"{"a":2,"b":3}"#, "first position, last value");
        assert_eq!(round_trip(r#""\uD800x""#), r#""\ud800x""#, "a lone surrogate round-trips");
        assert_eq!(round_trip(r#""\uD83D\uDE00""#), "\"\u{1F600}\"", "a pair is one character");
        for bad in ["NaN", "Infinity", "-Infinity", "{a:1}", "[1,]", "01"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        for (n, s) in [(1e-7, "1e-7"), (1e21, "1e+21"), (5e-324, "5e-324"), (1e-6, "0.000001")] {
            assert_eq!(stringify(&Value::Number(n)).unwrap(), s);
        }
        assert_eq!(stringify(&Value::Number(f64::NAN)).unwrap(), "null");
        let pretty = stringify_pretty(&parse(r#"{"a":[],"b":{},"c":[1]}"#).unwrap()).unwrap();
        assert_eq!(pretty, "{\n  \"a\": [],\n  \"b\": {},\n  \"c\": [\n    1\n  ]\n}");
    }
}

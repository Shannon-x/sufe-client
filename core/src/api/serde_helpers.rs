//! Lenient JSON deserializers shared across `api/*` modules.
//!
//! ## Why this module exists
//!
//! v2board / xboard panels — and the many third-party forks built on top of
//! them — do **not** guarantee a stable wire-level type for the fields they
//! expose. Two specific PHP-side patterns force us to be permissive on the
//! Rust side or the whole response fails to deserialize:
//!
//! 1. **MySQL `DECIMAL` without an Eloquent `$cast`.** PDO returns DECIMALs
//!    as PHP strings, so the JSON encoder emits `"0.00"` instead of `0.00`.
//!    (Hit by `payments.handling_fee_*`.)
//!
//! 2. **IEEE-754-lossy multiplies on cent-yuan prices.** `PlanResource`
//!    computes `(float) $price * 100`, so a stored yuan price of `19.99`
//!    becomes `1998.9999999999998`. Plain `Option<i64>` rejects that.
//!
//! 3. **`admin_setting()` returning bare `null` / `0` / `false` for unset
//!    keys.** `#[serde(default)]` only rescues a *missing* key — an explicit
//!    `null` into a non-Option `String` still fails. Same for `0` (int) into
//!    a `Vec<String>` when an admin toggles a whitelist off.
//!
//! Promoted from `api::order::de_num_or_string` so multiple `api::*` files
//! can share the same helpers without re-defining them.

use serde::de::{self, Deserializer, Visitor};
use std::fmt;
use std::marker::PhantomData;

// ---------------------------------------------------------------------- //
// Number-or-string (lenient i64 / f64 / f32 with null fallback)           //
// ---------------------------------------------------------------------- //

/// Sentinel returned by the inner visitor; the outer wrappers below convert
/// it into the concrete `Option<T>` shape the struct field actually wants.
enum NumOrString {
    None,
    Float(f64),
    Int(i64),
    UInt(u64),
    Str(String),
}

struct NumOrStringVisitor;

impl<'de> Visitor<'de> for NumOrStringVisitor {
    type Value = NumOrString;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a number, a numeric string, or null")
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(NumOrString::None)
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(NumOrString::None)
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(NumOrStringVisitor)
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        Ok(NumOrString::Float(v))
    }
    fn visit_f32<E: de::Error>(self, v: f32) -> Result<Self::Value, E> {
        Ok(NumOrString::Float(v as f64))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(NumOrString::Int(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(NumOrString::UInt(v))
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        // Some forks emit `false` for "no value" on numeric fields.
        Ok(NumOrString::Int(if v { 1 } else { 0 }))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Ok(NumOrString::Str(v.to_owned()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        Ok(NumOrString::Str(v))
    }
    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok(NumOrString::Str(v.to_owned()))
    }
}

/// Outer option visitor — lets us treat a missing key, `null`, or an explicit
/// value uniformly, since the field has `#[serde(default)]`.
struct OptVisitor<T>(PhantomData<T>);

impl<'de, T> Visitor<'de> for OptVisitor<T>
where
    T: FromNumOrString,
{
    type Value = Option<T>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a number, a numeric string, or null")
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        let raw = d.deserialize_any(NumOrStringVisitor)?;
        T::from_num_or_string(raw).map_err(de::Error::custom)
    }
    // Some self-describing formats (incl. serde_json on a bare value) call
    // `deserialize_any` straight through to these variants rather than
    // wrapping in `visit_some`; mirror the conversion.
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        T::from_num_or_string(NumOrString::Float(v)).map_err(de::Error::custom)
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        T::from_num_or_string(NumOrString::Int(v)).map_err(de::Error::custom)
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        T::from_num_or_string(NumOrString::UInt(v)).map_err(de::Error::custom)
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        T::from_num_or_string(NumOrString::Int(if v { 1 } else { 0 })).map_err(de::Error::custom)
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        T::from_num_or_string(NumOrString::Str(v.to_owned())).map_err(de::Error::custom)
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        T::from_num_or_string(NumOrString::Str(v)).map_err(de::Error::custom)
    }
}

trait FromNumOrString: Sized {
    fn from_num_or_string(raw: NumOrString) -> Result<Option<Self>, String>;
}

impl FromNumOrString for f64 {
    fn from_num_or_string(raw: NumOrString) -> Result<Option<Self>, String> {
        match raw {
            NumOrString::None => Ok(None),
            NumOrString::Float(n) => Ok(Some(n)),
            NumOrString::Int(n) => Ok(Some(n as f64)),
            NumOrString::UInt(n) => Ok(Some(n as f64)),
            NumOrString::Str(s) => {
                let t = s.trim();
                if t.is_empty() {
                    // Treat blank strings as "no value" rather than an
                    // error — older panels occasionally emit "".
                    Ok(None)
                } else {
                    t.parse::<f64>()
                        .map(Some)
                        .map_err(|e| format!("invalid f64 string {s:?}: {e}"))
                }
            }
        }
    }
}

impl FromNumOrString for f32 {
    fn from_num_or_string(raw: NumOrString) -> Result<Option<Self>, String> {
        match raw {
            NumOrString::None => Ok(None),
            NumOrString::Float(n) => Ok(Some(n as f32)),
            NumOrString::Int(n) => Ok(Some(n as f32)),
            NumOrString::UInt(n) => Ok(Some(n as f32)),
            NumOrString::Str(s) => {
                let t = s.trim();
                if t.is_empty() {
                    Ok(None)
                } else {
                    t.parse::<f32>()
                        .map(Some)
                        .map_err(|e| format!("invalid f32 string {s:?}: {e}"))
                }
            }
        }
    }
}

impl FromNumOrString for i64 {
    fn from_num_or_string(raw: NumOrString) -> Result<Option<Self>, String> {
        match raw {
            NumOrString::None => Ok(None),
            NumOrString::Int(n) => Ok(Some(n)),
            NumOrString::UInt(n) => {
                if n <= i64::MAX as u64 {
                    Ok(Some(n as i64))
                } else {
                    Err(format!("u64 {n} overflows i64"))
                }
            }
            // PlanResource emits `(float) $price * 100`, which yields lossy
            // IEEE-754 values like `1998.9999999999998` for 19.99 yuan.
            // Truncate toward the nearest cent — the panel's source value is
            // an integer cent count, so the fractional part is purely noise.
            // Also: DECIMAL(M,0) round-trips through serde_json as f64 in
            // builds that *do* cast — accept that too rather than bouncing
            // on a trivial type discrepancy.
            NumOrString::Float(n) => Ok(Some(n.round() as i64)),
            NumOrString::Str(s) => {
                let t = s.trim();
                if t.is_empty() {
                    Ok(None)
                } else if let Ok(n) = t.parse::<i64>() {
                    Ok(Some(n))
                } else if let Ok(n) = t.parse::<f64>() {
                    // Panels send "0.00" even for the integer fixed-fee
                    // column. Round to the nearest int — see Float branch.
                    Ok(Some(n.round() as i64))
                } else {
                    Err(format!("invalid i64 string {s:?}"))
                }
            }
        }
    }
}

/// Accepts a JSON number, numeric string, bool, or null; returns
/// `Option<f64>`. Pair with `#[serde(default)]`.
pub fn deserialize_opt_f64_lenient<'de, D>(d: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    d.deserialize_any(OptVisitor::<f64>(PhantomData))
}

/// Accepts a JSON number, numeric string, bool, or null; returns
/// `Option<f32>`. Pair with `#[serde(default)]`.
pub fn deserialize_opt_f32_lenient<'de, D>(d: D) -> Result<Option<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    d.deserialize_any(OptVisitor::<f32>(PhantomData))
}

/// Accepts a JSON int, float (rounded), numeric string, bool, or null;
/// returns `Option<i64>`. Pair with `#[serde(default)]`. Tolerates the
/// `(float) $cents * 100` IEEE-754 drift documented at the top of this file.
pub fn deserialize_opt_i64_lenient<'de, D>(d: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    d.deserialize_any(OptVisitor::<i64>(PhantomData))
}

// ---------------------------------------------------------------------- //
// String-or-null (admin_setting() unset keys)                             //
// ---------------------------------------------------------------------- //

/// Accepts a JSON string, null, or missing key; maps any non-string to `""`.
/// Pair with `#[serde(default)]`. Use this for fields whose backend source
/// is `admin_setting('foo')` with no default — Laravel returns `null` for
/// unconfigured keys, which serde would otherwise refuse to coerce into a
/// non-Option `String`.
pub fn de_string_or_null<'de, D>(d: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a string, null, or missing")
        }
        fn visit_none<E: de::Error>(self) -> Result<String, E> {
            Ok(String::new())
        }
        fn visit_unit<E: de::Error>(self) -> Result<String, E> {
            Ok(String::new())
        }
        fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<String, D::Error> {
            d.deserialize_any(V)
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_owned())
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<String, E> {
            Ok(v)
        }
        fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<String, E> {
            Ok(v.to_owned())
        }
        // Some panels emit `false` / `0` for an unset string-typed setting;
        // collapse all non-string scalars to empty rather than erroring.
        fn visit_bool<E: de::Error>(self, _v: bool) -> Result<String, E> {
            Ok(String::new())
        }
        fn visit_i64<E: de::Error>(self, _v: i64) -> Result<String, E> {
            Ok(String::new())
        }
        fn visit_u64<E: de::Error>(self, _v: u64) -> Result<String, E> {
            Ok(String::new())
        }
        fn visit_f64<E: de::Error>(self, _v: f64) -> Result<String, E> {
            Ok(String::new())
        }
    }
    d.deserialize_any(V)
}

// ---------------------------------------------------------------------- //
// Vec<String>-or-empty (admin_setting() toggled-off list)                 //
// ---------------------------------------------------------------------- //

/// Accepts a JSON array of strings, or null / `0` / `false` / missing /
/// any non-array scalar; returns `Vec::new()` for the non-array cases.
/// Pair with `#[serde(default)]`. The motivating case is `CommController`
/// emitting literal int `0` for `email_whitelist_suffix` when the toggle
/// is off; `#[serde(default)]` alone does not rescue an explicit `0` value
/// for a `Vec<String>`-typed field.
pub fn de_vec_string_or_empty<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an array of strings, null, or a falsy scalar")
        }
        fn visit_none<E: de::Error>(self) -> Result<Vec<String>, E> {
            Ok(Vec::new())
        }
        fn visit_unit<E: de::Error>(self) -> Result<Vec<String>, E> {
            Ok(Vec::new())
        }
        fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Vec<String>, D::Error> {
            d.deserialize_any(V)
        }
        fn visit_bool<E: de::Error>(self, _v: bool) -> Result<Vec<String>, E> {
            Ok(Vec::new())
        }
        fn visit_i64<E: de::Error>(self, _v: i64) -> Result<Vec<String>, E> {
            Ok(Vec::new())
        }
        fn visit_u64<E: de::Error>(self, _v: u64) -> Result<Vec<String>, E> {
            Ok(Vec::new())
        }
        fn visit_f64<E: de::Error>(self, _v: f64) -> Result<Vec<String>, E> {
            Ok(Vec::new())
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Vec<String>, E> {
            // Defensive: some forks JSON-encode the list a second time and
            // ship it as a string. Try to decode; on failure fall back to a
            // single-element vec (trimmed) if the string looks meaningful,
            // otherwise empty.
            let t = v.trim();
            if t.is_empty() {
                return Ok(Vec::new());
            }
            if let Ok(parsed) = serde_json::from_str::<Vec<String>>(t) {
                return Ok(parsed);
            }
            Ok(vec![t.to_owned()])
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<Vec<String>, E> {
            self.visit_str(&v)
        }
        fn visit_seq<A>(self, mut seq: A) -> Result<Vec<String>, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            while let Some(elem) = seq.next_element::<serde_json::Value>()? {
                match elem {
                    serde_json::Value::String(s) => out.push(s),
                    serde_json::Value::Null => {}
                    other => out.push(other.to_string()),
                }
            }
            Ok(out)
        }
    }
    d.deserialize_any(V)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct PriceProbe {
        #[serde(default, deserialize_with = "deserialize_opt_i64_lenient")]
        price: Option<i64>,
    }

    #[test]
    fn opt_i64_accepts_lossy_float() {
        // `(float) 1999 / 100 * 100` in PHP — what PlanResource ships for a
        // 19.99 yuan plan.
        let r: PriceProbe = serde_json::from_str(r#"{"price": 1998.9999999999998}"#).unwrap();
        assert_eq!(r.price, Some(1999));
    }

    #[test]
    fn opt_i64_accepts_integer_string() {
        let r: PriceProbe = serde_json::from_str(r#"{"price": "2500"}"#).unwrap();
        assert_eq!(r.price, Some(2500));
    }

    #[test]
    fn opt_i64_accepts_null_and_missing() {
        let a: PriceProbe = serde_json::from_str(r#"{"price": null}"#).unwrap();
        let b: PriceProbe = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(a.price, None);
        assert_eq!(b.price, None);
    }

    #[derive(Deserialize)]
    struct StrProbe {
        #[serde(default, deserialize_with = "de_string_or_null")]
        s: String,
    }

    #[test]
    fn string_or_null_handles_null() {
        let r: StrProbe = serde_json::from_str(r#"{"s": null}"#).unwrap();
        assert_eq!(r.s, "");
    }

    #[test]
    fn string_or_null_handles_missing() {
        let r: StrProbe = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(r.s, "");
    }

    #[test]
    fn string_or_null_passes_through_real_string() {
        let r: StrProbe = serde_json::from_str(r#"{"s": "hi"}"#).unwrap();
        assert_eq!(r.s, "hi");
    }

    #[derive(Deserialize)]
    struct VecProbe {
        #[serde(default, deserialize_with = "de_vec_string_or_empty")]
        v: Vec<String>,
    }

    #[test]
    fn vec_string_or_empty_handles_zero() {
        // The exact CommController shape when the toggle is off.
        let r: VecProbe = serde_json::from_str(r#"{"v": 0}"#).unwrap();
        assert!(r.v.is_empty());
    }

    #[test]
    fn vec_string_or_empty_handles_null_and_missing() {
        let a: VecProbe = serde_json::from_str(r#"{"v": null}"#).unwrap();
        let b: VecProbe = serde_json::from_str(r#"{}"#).unwrap();
        assert!(a.v.is_empty());
        assert!(b.v.is_empty());
    }

    #[test]
    fn vec_string_or_empty_passes_through_array() {
        let r: VecProbe = serde_json::from_str(r#"{"v": ["@a.com", "@b.com"]}"#).unwrap();
        assert_eq!(r.v, vec!["@a.com", "@b.com"]);
    }

    #[derive(Deserialize)]
    struct F32Probe {
        #[serde(default, deserialize_with = "deserialize_opt_f32_lenient")]
        t: Option<f32>,
    }

    #[test]
    fn opt_f32_handles_string_and_number() {
        let a: F32Probe = serde_json::from_str(r#"{"t": "0.5"}"#).unwrap();
        let b: F32Probe = serde_json::from_str(r#"{"t": 0.7}"#).unwrap();
        let c: F32Probe = serde_json::from_str(r#"{"t": null}"#).unwrap();
        let d: F32Probe = serde_json::from_str(r#"{"t": ""}"#).unwrap();
        assert_eq!(a.t, Some(0.5));
        assert_eq!(b.t, Some(0.7));
        assert_eq!(c.t, None);
        assert_eq!(d.t, None);
    }
}

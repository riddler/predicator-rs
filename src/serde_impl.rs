//! `Serialize` and `Deserialize` behind the `serde` feature: a [`Value`] in
//! the tagged encoding, a [`Date`] and a [`DateTime`] as the text that
//! encoding carries them in, and a [`Duration`] as its unit map.
//!
//! Both directions hold to the value boundary: serializing refuses what
//! [`tagged::encode`](crate::tagged::encode) refuses, and deserializing
//! refuses what [`tagged::decode`](crate::tagged::decode) refuses, each as a
//! custom error whose text opens with the reason. Deserializing walks no
//! deeper than 100 levels, so a deserializer that does not bound its own
//! nesting cannot exhaust the stack here. An integer a deserializer hands
//! over as a float (as `serde_json` does past the unsigned 64-bit range) is
//! a float.

use crate::boundary::{self, DEPTH_LIMIT};
use crate::json::{self, Refusal};
use crate::tagged;
use crate::{Date, DateTime, Duration, Key, Map, Value};
use core::fmt;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::ser::{self, SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

fn text(refusal: Refusal) -> String {
    format!("{}: {}", refusal.reason, refusal.message)
}

/// Serializes a tag: an object with `$type` and, when given, `value`.
fn tag<S: Serializer, T: Serialize + ?Sized>(
    serializer: S,
    name: &str,
    value: Option<&T>,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(1 + usize::from(value.is_some())))?;
    map.serialize_entry("$type", name)?;
    if let Some(value) = value {
        map.serialize_entry("value", value)?;
    }
    map.end()
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        boundary::check(self).map_err(|error| {
            ser::Error::custom(format!("{}: {}", error.reason(), error.message()))
        })?;
        Checked(self).serialize(serializer)
    }
}

/// A value the boundary has passed, so its nesting is bounded.
struct Checked<'a>(&'a Value);

impl Serialize for Checked<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Null => serializer.serialize_unit(),
            Value::Undefined => tag::<S, str>(serializer, "undefined", None),
            Value::Bool(flag) => serializer.serialize_bool(*flag),
            Value::Integer(number) => serializer.serialize_i64(*number),
            Value::Float(number) => serializer.serialize_f64(*number),
            Value::String(text) => serializer.serialize_str(text),
            Value::List(items) => {
                let mut list = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    list.serialize_element(&Checked(item))?;
                }
                list.end()
            }
            Value::Map(map) => {
                tagged::check_map(map).map_err(|refusal| ser::Error::custom(text(refusal)))?;
                let mut entries = serializer.serialize_map(Some(map.len()))?;
                for (key, item) in map.iter() {
                    let Key::String(key) = key else {
                        return Err(ser::Error::custom(text(json::non_string_key(key))));
                    };
                    entries.serialize_entry(key, &Checked(item))?;
                }
                entries.end()
            }
            Value::Date(date) => tag(serializer, "date", Some(date)),
            Value::DateTime(instant) => tag(serializer, "datetime", Some(instant)),
            Value::Duration(duration) => tag(serializer, "duration", Some(duration)),
        }
    }
}

impl Serialize for Date {
    /// The date's text, `YYYY-MM-DD`; a year outside 0 to 9999 is refused.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let carried = tagged::carried(self.year(), self.to_string())
            .map_err(|refusal| ser::Error::custom(text(refusal)))?;
        serializer.serialize_str(&carried)
    }
}

impl Serialize for DateTime {
    /// The instant's canonical text in UTC; a year outside 0 to 9999 is
    /// refused.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let carried = tagged::carried(self.date().year(), self.to_string())
            .map_err(|refusal| ser::Error::custom(text(refusal)))?;
        serializer.serialize_str(&carried)
    }
}

impl Serialize for Duration {
    /// The unit map in key order, `milliseconds` only when it is not zero.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut units: Vec<(&str, i64)> = self
            .units()
            .into_iter()
            .filter(|(name, _, amount)| *name != "milliseconds" || *amount != 0)
            .map(|(name, _, amount)| (name, amount))
            .collect();
        units.sort_unstable_by_key(|(name, _)| *name);
        let mut map = serializer.serialize_map(Some(units.len()))?;
        for (name, amount) in units {
            map.serialize_entry(name, &amount)?;
        }
        map.end()
    }
}

fn depth<E: de::Error>(depth: usize) -> Result<(), E> {
    if depth > DEPTH_LIMIT {
        return Err(E::custom(format!(
            "depth_limit_exceeded: the value nests past {DEPTH_LIMIT} levels"
        )));
    }
    Ok(())
}

/// Reads a value at a depth, the outermost list or map being the first.
struct ValueSeed(usize);

impl<'de> DeserializeSeed<'de> for ValueSeed {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ValueSeed {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a predicator value in the tagged encoding")
    }

    fn visit_bool<E>(self, flag: bool) -> Result<Value, E> {
        Ok(Value::Bool(flag))
    }

    fn visit_i64<E>(self, number: i64) -> Result<Value, E> {
        Ok(Value::Integer(number))
    }

    fn visit_u64<E: de::Error>(self, number: u64) -> Result<Value, E> {
        i64::try_from(number).map(Value::Integer).map_err(|_| {
            E::custom(format!(
                "integer_out_of_range: the integer {number} is outside the 64-bit signed range"
            ))
        })
    }

    fn visit_f64<E: de::Error>(self, number: f64) -> Result<Value, E> {
        if number.is_finite() {
            Ok(Value::Float(number))
        } else {
            Err(E::custom(format!(
                "non_finite_number: the float {number} is not finite"
            )))
        }
    }

    fn visit_str<E>(self, text: &str) -> Result<Value, E> {
        Ok(Value::String(text.to_owned()))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<Value, A::Error> {
        depth(self.0)?;
        let mut list = Vec::new();
        while let Some(item) = items.next_element_seed(ValueSeed(self.0 + 1))? {
            list.push(item);
        }
        Ok(Value::List(list))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Value, A::Error> {
        depth(self.0)?;
        let mut map = Map::new();
        while let Some(key) = entries.next_key_seed(KeySeed)? {
            let item = entries.next_value_seed(ValueSeed(self.0 + 1))?;
            let key = Key::String(key);
            if !map.contains_key(&key) {
                map.insert(key, item);
            }
        }
        tagged::untag(map).map_err(|refusal| de::Error::custom(text(refusal)))
    }
}

/// Reads a map key, which is a string.
struct KeySeed;

impl<'de> DeserializeSeed<'de> for KeySeed {
    type Value = String;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for KeySeed {
    type Value = String;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a string key")
    }

    fn visit_str<E>(self, key: &str) -> Result<String, E> {
        Ok(key.to_owned())
    }
}

impl<'de> Deserialize<'de> for Value {
    /// Reads a value in the tagged encoding.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
        ValueSeed(1).deserialize(deserializer)
    }
}

/// Reads the text of a date or an instant with `parse`.
struct TextVisitor<T>(fn(&str) -> Option<T>, &'static str);

impl<T> Visitor<'_> for TextVisitor<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.1)
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<T, E> {
        (self.0)(text)
            .ok_or_else(|| E::custom(format!("invalid_tagged_value: {text:?} is not {}", self.1)))
    }
}

impl<'de> Deserialize<'de> for Date {
    /// Reads `YYYY-MM-DD` with a four-digit year.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Date, D::Error> {
        deserializer.deserialize_str(TextVisitor(Date::parse, "an ISO 8601 date"))
    }
}

impl<'de> Deserialize<'de> for DateTime {
    /// Reads an ISO 8601 instant as [`tagged::decode`] reads one.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<DateTime, D::Error> {
        deserializer.deserialize_str(TextVisitor(DateTime::parse, "an ISO 8601 instant"))
    }
}

impl<'de> Deserialize<'de> for Duration {
    /// Reads the unit map: the seven units other than `milliseconds`
    /// always, `milliseconds` when present, each an integer, nothing else.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        match &ValueSeed(1).deserialize(deserializer)? {
            Value::Map(units) => tagged::duration(units),
            _ => None,
        }
        .ok_or_else(|| de::Error::custom("invalid_tagged_value: not a duration's unit map"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::IntoDeserializer;

    fn date(year: i32, month: u32, day: u32) -> Date {
        Date::new(year, month, day).unwrap()
    }

    fn samples() -> Vec<Value> {
        let instant = DateTime::new(date(2026, 8, 9), 10, 30, 0, 500_000).unwrap();
        vec![
            Value::Null,
            Value::Undefined,
            Value::from(true),
            Value::from(-7),
            Value::from(2.5),
            Value::from("x"),
            Value::from(vec![Value::from(1), Value::Null]),
            Value::from([("a", Value::from(1))].into_iter().collect::<Map>()),
            Value::from(date(2026, 8, 9)),
            Value::from(instant),
            Value::from(Duration {
                days: 3,
                milliseconds: 5,
                ..Duration::default()
            }),
        ]
    }

    // Sabotage: with Checked serializing Undefined as a unit, this test
    // fails.
    #[test]
    fn serde_writes_and_reads_the_tagged_encoding() {
        for value in samples() {
            let text = serde_json::to_string(&value).unwrap();
            assert_eq!(text, tagged::encode(&value).unwrap());
            assert_eq!(
                serde_json::from_str::<Value>(&text).unwrap(),
                value,
                "{text}"
            );
        }
        let first: Value = serde_json::from_str(r#"{"a":1,"a":2}"#).unwrap();
        assert_eq!(
            first,
            Value::from([("a", Value::from(1))].into_iter().collect::<Map>())
        );
    }

    // Sabotage: with ValueSeed's depth test written depth > DEPTH_LIMIT + 1,
    // this test fails.
    #[test]
    fn serde_refuses_what_the_codec_refuses() {
        let failure = |value: Value| serde_json::to_string(&value).unwrap_err().to_string();
        assert!(failure(Value::from(f64::NAN)).starts_with("non_finite_number"));
        let reserved: Map = [("$type", Value::Null)].into_iter().collect();
        assert!(failure(reserved.into()).starts_with("reserved_map_key"));
        let integer_key: Map = [(Key::from(1), Value::Null)].into_iter().collect();
        assert!(failure(integer_key.into()).starts_with("non_string_map_key"));
        assert!(failure(Value::from(date(10_000, 1, 1))).starts_with("invalid_tagged_value"));
        let mut deep = Value::Null;
        for _ in 0..101 {
            deep = Value::from(vec![deep]);
        }
        assert!(failure(deep).starts_with("depth_limit_exceeded"));
        let read = |text: &str| serde_json::from_str::<Value>(text).unwrap_err().to_string();
        assert!(
            read(&format!("{}{}", "[".repeat(101), "]".repeat(101)))
                .starts_with("depth_limit_exceeded")
        );
        assert!(
            serde_json::from_str::<Value>(&format!("{}{}", "[".repeat(100), "]".repeat(100)))
                .is_ok()
        );
        assert!(read("18446744073709551615").starts_with("integer_out_of_range"));
        assert!(read(r#"{"$type":"nope"}"#).starts_with("invalid_tagged_value"));
        assert!(read("{1:2}").contains("key must be a string"));
        let infinite: Result<Value, de::value::Error> =
            Value::deserialize(f64::INFINITY.into_deserializer());
        assert!(
            infinite
                .unwrap_err()
                .to_string()
                .starts_with("non_finite_number")
        );
        let unsigned: Result<Value, de::value::Error> =
            Value::deserialize(7_u64.into_deserializer());
        assert_eq!(unsigned.unwrap(), Value::from(7));
        let key: Result<Value, de::value::Error> = Value::deserialize(
            serde::de::value::MapDeserializer::new([(1_u8, 2_u8)].into_iter()),
        );
        assert!(key.unwrap_err().to_string().contains("a string key"));
    }

    // Sabotage: with Duration's Serialize writing a zero milliseconds,
    // this test fails.
    #[test]
    fn the_value_types_carry_their_own_text() {
        let day = date(2026, 8, 9);
        assert_eq!(serde_json::to_string(&day).unwrap(), r#""2026-08-09""#);
        assert_eq!(
            serde_json::from_str::<Date>(r#""2026-08-09""#).unwrap(),
            day
        );
        assert!(serde_json::from_str::<Date>(r#""2026-02-30""#).is_err());
        assert!(serde_json::to_string(&date(-1, 1, 1)).is_err());
        let instant = DateTime::new(day, 1, 2, 3, 0).unwrap();
        assert_eq!(
            serde_json::to_string(&instant).unwrap(),
            r#""2026-08-09T01:02:03Z""#
        );
        assert_eq!(
            serde_json::from_str::<DateTime>(r#""2026-08-09T02:02:03+01:00""#).unwrap(),
            instant
        );
        let refused = serde_json::from_str::<DateTime>("\"x\"")
            .unwrap_err()
            .to_string();
        assert!(refused.starts_with("invalid_tagged_value"), "{refused}");
        assert!(
            serde_json::from_str::<DateTime>("1")
                .unwrap_err()
                .to_string()
                .contains("an ISO 8601 instant")
        );
        assert!(
            serde_json::to_string(&DateTime::new(date(10_000, 1, 1), 0, 0, 0, 0).unwrap()).is_err()
        );
        let duration = Duration {
            weeks: 1,
            ..Duration::default()
        };
        let text = r#"{"days":0,"hours":0,"minutes":0,"months":0,"seconds":0,"weeks":1,"years":0}"#;
        assert_eq!(serde_json::to_string(&duration).unwrap(), text);
        assert_eq!(serde_json::from_str::<Duration>(text).unwrap(), duration);
        assert!(serde_json::from_str::<Duration>(r#"{"days":0}"#).is_err());
        assert!(serde_json::from_str::<Duration>("[]").is_err());
    }
}

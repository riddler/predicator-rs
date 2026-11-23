//! The crate's own map: string keys in codepoint order, over a `BTreeMap`.

use crate::Value;
use std::collections::BTreeMap;

/// A predicator map: string keys, each with a [`Value`], kept in codepoint
/// order of the keys (the order the tagged encoding writes them in).
///
/// A key is a string, as a key of a JSON object is.
///
/// ```
/// use predicator_rs::{Map, Value};
///
/// let mut map = Map::new();
/// map.insert("b", Value::Integer(2));
/// map.insert("a", Value::Integer(1));
/// assert_eq!(map.len(), 2);
/// assert_eq!(map.get("a"), Some(&Value::Integer(1)));
/// assert_eq!(map.keys().collect::<Vec<_>>(), ["a", "b"]);
/// ```
#[derive(Clone, Default, PartialEq)]
pub struct Map {
    pub(crate) entries: BTreeMap<String, Value>,
}

impl Map {
    /// An empty map.
    pub fn new() -> Map {
        Map::default()
    }

    /// How many keys the map holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map holds no keys.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The value at `key`, if the map holds it.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.get(key)
    }

    /// The value at `key`, mutably, if the map holds it.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.entries.get_mut(key)
    }

    /// Whether the map holds `key`.
    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Puts `value` at `key`, answering the value it replaces.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) -> Option<Value> {
        self.entries.insert(key.into(), value)
    }

    /// Takes the value at `key` out of the map.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.entries.remove(key)
    }

    /// The keys and values, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }

    /// The keys, in order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// The values, in key order.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.values()
    }
}

impl std::fmt::Debug for Map {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.entries.iter()).finish()
    }
}

impl<K: Into<String>> FromIterator<(K, Value)> for Map {
    /// A map of the pairs; a key given twice keeps its last value.
    fn from_iter<I: IntoIterator<Item = (K, Value)>>(pairs: I) -> Map {
        Map {
            entries: pairs
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        }
    }
}

impl IntoIterator for Map {
    type Item = (String, Value);
    type IntoIter = std::collections::btree_map::IntoIter<String, Value>;

    /// The keys and values, by value, in key order.
    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sabotage: with Map::remove answering None without removing, this
    // test fails.
    #[test]
    fn a_map_keeps_string_keys_in_codepoint_order() {
        let mut map: Map = [("é", Value::Null), ("z", Value::Null), ("a", Value::Null)]
            .into_iter()
            .collect();
        assert_eq!(map.keys().collect::<Vec<_>>(), ["a", "z", "é"]);
        assert!(!map.is_empty());
        assert!(map.contains_key("z"));
        assert_eq!(map.insert("a", Value::Bool(true)), Some(Value::Null));
        *map.get_mut("z").unwrap() = Value::Integer(1);
        assert_eq!(map.remove("é"), Some(Value::Null));
        assert_eq!(map.remove("é"), None);
        assert_eq!(
            map.iter().collect::<Vec<_>>(),
            [("a", &Value::Bool(true)), ("z", &Value::Integer(1))]
        );
        assert_eq!(map.values().count(), 2);
        assert_eq!(format!("{map:?}"), r#"{"a": Bool(true), "z": Integer(1)}"#);
        let owned: Vec<(String, Value)> = map.into_iter().collect();
        assert_eq!(owned.len(), 2);
        assert!(Map::new().is_empty());
    }
}

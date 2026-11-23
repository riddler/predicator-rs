//! The crate's own map, over a `BTreeMap`, and its key.

use crate::Value;
use std::collections::BTreeMap;

/// A map key: one of the four kinds the reference's instruction-set text
/// lets index a map, a string, an integer, a boolean or undefined. (`null`
/// is not one: the text makes a `null` key a type mismatch.)
///
/// Keys are totally ordered: undefined first, then `false` and `true`, then
/// integers in numeric order, then strings in codepoint order. A map keeps
/// its keys in this order.
///
/// ```
/// use predicator_rs::Key;
///
/// let mut keys = vec![Key::from("a"), Key::from(2), Key::from(true), Key::Undefined];
/// keys.sort();
/// assert_eq!(keys, [Key::Undefined, Key::from(true), Key::from(2), Key::from("a")]);
/// assert_eq!(format!("{:?}", keys), r#"[Undefined, true, 2, "a"]"#);
/// ```
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    /// The undefined key.
    Undefined,
    /// `false` or `true`.
    Bool(bool),
    /// An integer.
    Integer(i64),
    /// A string.
    String(String),
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Key::Undefined => f.write_str("Undefined"),
            Key::Bool(flag) => write!(f, "{flag}"),
            Key::Integer(number) => write!(f, "{number}"),
            Key::String(text) => write!(f, "{text:?}"),
        }
    }
}

impl From<&str> for Key {
    fn from(text: &str) -> Key {
        Key::String(text.to_owned())
    }
}

impl From<String> for Key {
    fn from(text: String) -> Key {
        Key::String(text)
    }
}

impl From<i64> for Key {
    fn from(number: i64) -> Key {
        Key::Integer(number)
    }
}

impl From<bool> for Key {
    fn from(flag: bool) -> Key {
        Key::Bool(flag)
    }
}

/// A predicator map: [`Key`]s, each with a [`Value`], kept in key order.
///
/// ```
/// use predicator_rs::{Key, Map, Value};
///
/// let mut map = Map::new();
/// map.insert("b", Value::Integer(2));
/// map.insert("a", Value::Integer(1));
/// map.insert(true, Value::Null);
/// assert_eq!(map.len(), 3);
/// assert_eq!(map.get(&"a".into()), Some(&Value::Integer(1)));
/// assert_eq!(map.get(&Key::Integer(1)), None);
/// assert_eq!(
///     map.keys().collect::<Vec<_>>(),
///     [&Key::from(true), &Key::from("a"), &Key::from("b")]
/// );
/// ```
#[derive(Clone, Default, PartialEq)]
pub struct Map {
    pub(crate) entries: BTreeMap<Key, Value>,
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
    pub fn get(&self, key: &Key) -> Option<&Value> {
        self.entries.get(key)
    }

    /// The value at `key`, mutably, if the map holds it.
    pub fn get_mut(&mut self, key: &Key) -> Option<&mut Value> {
        self.entries.get_mut(key)
    }

    /// Whether the map holds `key`.
    pub fn contains_key(&self, key: &Key) -> bool {
        self.entries.contains_key(key)
    }

    /// Puts `value` at `key`, answering the value it replaces.
    pub fn insert(&mut self, key: impl Into<Key>, value: Value) -> Option<Value> {
        self.entries.insert(key.into(), value)
    }

    /// Takes the value at `key` out of the map.
    pub fn remove(&mut self, key: &Key) -> Option<Value> {
        self.entries.remove(key)
    }

    /// The keys and values, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&Key, &Value)> {
        self.entries.iter()
    }

    /// The keys, in order.
    pub fn keys(&self) -> impl Iterator<Item = &Key> {
        self.entries.keys()
    }

    /// The values, in key order.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.values()
    }

    /// The value at the string key `key`.
    pub(crate) fn get_str(&self, key: &str) -> Option<&Value> {
        self.entries.get(&Key::String(key.to_owned()))
    }
}

impl std::fmt::Debug for Map {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.entries.iter()).finish()
    }
}

impl<K: Into<Key>> FromIterator<(K, Value)> for Map {
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
    type Item = (Key, Value);
    type IntoIter = std::collections::btree_map::IntoIter<Key, Value>;

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
    fn a_map_keeps_its_keys_in_key_order() {
        let mut map: Map = [("é", Value::Null), ("z", Value::Null), ("a", Value::Null)]
            .into_iter()
            .collect();
        map.insert(Key::Undefined, Value::Null);
        map.insert(-3, Value::Null);
        map.insert(false, Value::Null);
        assert_eq!(
            map.keys().collect::<Vec<_>>(),
            [
                &Key::Undefined,
                &Key::from(false),
                &Key::from(-3),
                &Key::from("a"),
                &Key::from("z"),
                &Key::from("é")
            ]
        );
        assert!(!map.is_empty());
        assert!(map.contains_key(&"z".into()));
        assert!(!map.contains_key(&Key::from(true)));
        assert_eq!(map.insert("a", Value::Bool(true)), Some(Value::Null));
        *map.get_mut(&"z".into()).unwrap() = Value::Integer(1);
        assert_eq!(map.get_str("z"), Some(&Value::Integer(1)));
        assert_eq!(map.get_str("q"), None);
        assert_eq!(map.remove(&"é".into()), Some(Value::Null));
        assert_eq!(map.remove(&"é".into()), None);
        for key in [Key::Undefined, Key::from(false), Key::from(-3)] {
            map.remove(&key);
        }
        assert_eq!(
            map.iter().collect::<Vec<_>>(),
            [
                (&Key::from("a"), &Value::Bool(true)),
                (&Key::from("z"), &Value::Integer(1))
            ]
        );
        assert_eq!(map.values().count(), 2);
        assert_eq!(format!("{map:?}"), r#"{"a": Bool(true), "z": Integer(1)}"#);
        let owned: Vec<(Key, Value)> = map.into_iter().collect();
        assert_eq!(owned.len(), 2);
        assert!(Map::new().is_empty());
    }

    // Sabotage: with Key's variants declared strings first, this test fails.
    #[test]
    fn keys_order_undefined_booleans_integers_then_strings() {
        let mut keys = vec![
            Key::from("b"),
            Key::from(i64::MAX),
            Key::from("A"),
            Key::from(true),
            Key::from(i64::MIN),
            Key::Undefined,
            Key::from(false),
            Key::from(String::from("a")),
        ];
        keys.sort();
        assert_eq!(
            format!("{keys:?}"),
            format!(
                r#"[Undefined, false, true, {}, {}, "A", "a", "b"]"#,
                i64::MIN,
                i64::MAX
            )
        );
    }
}

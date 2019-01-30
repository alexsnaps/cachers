// Copyright 2018 Alex Snaps
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::eviction::ClockEvictor;
use crate::eviction::Evictor;
use std;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ops::Fn;
use std::sync::RwLock;

pub struct Segment<K, V> {
  data: HashMap<K, CacheEntry<V>>,
  evictor: ClockEvictor<K>,
}

enum CacheEntry<V> {
  Value(CacheValue<V>),
  Lock(SoftLock<V>),
}

struct CacheValue<V> {
  value: V,
  index: usize,
}

struct SoftLock<V> {
  value: RwLock<Option<CacheValue<V>>>,
}

impl<K, V> Segment<K, V>
where
  K: std::cmp::Eq + std::hash::Hash + Copy,
  V: Clone,
{
  pub fn new(capacity: usize) -> Segment<K, V> {
    Segment {
      data: HashMap::new(),
      evictor: ClockEvictor::new(capacity),
    }
  }

  pub fn get(&self, key: &K) -> Option<V> {
    if let Some(cache_entry) = self.data.get(key) {
      match cache_entry {
        CacheEntry::Value(entry) => {
          self.evictor.touch(entry.index);
          return Some(entry.value.clone());
        }
        CacheEntry::Lock(_) => panic!("Fix me!"), // todo Handle lock
      }
    }
    None
  }

  pub fn update<F>(&mut self, key: K, updating_fn: F) -> Option<V>
  where
    F: Fn(&K, Option<V>) -> Option<V>,
  {
    let (remove_entry, option, key_evicted) = match self.data.entry(key) {
      Entry::Occupied(mut entry) => {
        let cache_value = match entry.get_mut() {
          CacheEntry::Value(entry) => entry,
          CacheEntry::Lock(_) => panic!("Fix me!"), // todo Handle lock
        };

        match updating_fn(&key, Some(cache_value.value.clone())) {
          Some(value) => {
            cache_value.value = value;
            self.evictor.touch(cache_value.index);
            (false, Some(cache_value.value.clone()), None)
          }
          None => (true, None, None),
        }
      }
      Entry::Vacant(entry) => {
        let (option, key_evicted) = match updating_fn(entry.key(), None) {
          Some(value) => {
            let (index, to_remove) = self.evictor.add(*entry.key());
            let cache_entry = entry.insert(CacheEntry::Value(CacheValue { value, index }));
            let entry = match cache_entry {
              CacheEntry::Value(entry) => entry,
              CacheEntry::Lock(_) => panic!("Fix me!"), // todo Handle lock
            };
            (Some(entry.value.clone()), to_remove)
          }
          None => (None, None),
        };
        (false, option, key_evicted)
      }
    };

    if remove_entry {
      self.data.remove(&key);
    }

    if key_evicted.is_some() {
      self.data.remove(&key_evicted.unwrap());
    }

    match option {
      Some(cache_entry) => Some(cache_entry.clone()),
      None => None,
    }
  }

  pub fn get_or_populate<F>(&mut self, key: K, populating_fn: F) -> Option<V>
  where
    F: Fn(&K) -> Option<V>,
  {
    let (option, key_evicted) = match self.data.entry(key) {
      Entry::Occupied(entry) => match entry.get() {
        CacheEntry::Value(entry) => {
          self.evictor.touch(entry.index);
          (Some(entry.value.clone()), None)
        }
        CacheEntry::Lock(_) => panic!("Fix me!"), // todo Handle lock
      },
      Entry::Vacant(entry) => {
        let (option, to_remove) = match populating_fn(entry.key()) {
          Some(value) => {
            let (index, to_remove) = self.evictor.add(entry.key().clone());
            let cache_entry = entry.insert(CacheEntry::Value(CacheValue { value, index }));
            match cache_entry {
              CacheEntry::Value(entry) => (Some(entry.value.clone()), to_remove),
              CacheEntry::Lock(_) => panic!("Fix me!"), // todo Handle lock
            }
          }
          None => (None, None),
        };
        (option, to_remove)
      }
    };

    if key_evicted.is_some() {
      self.data.remove(&key_evicted.unwrap());
    }

    option
  }

  #[cfg(test)]
  pub fn len(&self) -> usize {
    self.data.len()
  }
}

#[cfg(test)]
mod tests {
  use super::Segment;

  fn test_segment() -> Segment<i32, String> {
    Segment::new(3)
  }

  #[test]
  fn hit_populates() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;
    {
      let value = segment.get_or_populate(our_key, populate);
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.get_or_populate(our_key, do_not_invoke);
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }
  }

  #[test]
  fn miss_populates_not() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;
    {
      let value = segment.get_or_populate(our_key, miss);
      assert_eq!(value, None);
      assert_eq!(segment.len(), 0);
    }
  }

  #[test]
  fn get_or_populate_evicts() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;
    {
      let value = segment.get_or_populate(our_key, populate);
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
      segment.get_or_populate(2, populate);
      segment.get_or_populate(3, populate);
      assert_eq!(segment.len(), 3);
      segment.get_or_populate(4, populate);
      assert_eq!(segment.len(), 3);
    }
  }

  #[test]
  fn update_populates() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;

    {
      let value = segment.update(our_key, upsert);
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.get_or_populate(our_key, do_not_invoke);
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }
  }

  #[test]
  fn update_updates() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;

    {
      let value = segment.get_or_populate(our_key, populate);
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.update(our_key, update);
      assert_eq!(value.unwrap(), "42 updated!");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.get_or_populate(our_key, do_not_invoke);
      assert_eq!(value.unwrap(), "42 updated!");
      assert_eq!(segment.len(), 1);
    }
  }

  #[test]
  fn update_evicts() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;
    {
      let value = segment.update(our_key, upsert);
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
      segment.update(2, upsert);
      segment.update(3, upsert);
      assert_eq!(segment.len(), 3);
      segment.update(4, upsert);
      assert_eq!(segment.len(), 3);
    }
  }

  #[test]
  fn update_removes() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;

    {
      let value = segment.get_or_populate(our_key, populate);
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.update(our_key, updel);
      assert_eq!(value, None);
      assert_eq!(segment.len(), 0);
    }

    {
      let value = segment.get_or_populate(our_key, miss);
      assert_eq!(value, None);
      assert_eq!(segment.len(), 0);
    }
  }

  fn miss(_key: &i32) -> Option<String> {
    None
  }

  fn populate(key: &i32) -> Option<String> {
    Some(key.to_string())
  }

  fn upsert(key: &i32, value: Option<String>) -> Option<String> {
    assert_eq!(value, None);
    populate(key)
  }

  fn update(_key: &i32, value: Option<String>) -> Option<String> {
    let previous = &value.unwrap();
    Some(previous.clone() + " updated!")
  }

  fn updel(_key: &i32, value: Option<String>) -> Option<String> {
    assert!(value.is_some());
    None
  }

  fn do_not_invoke(_key: &i32) -> Option<String> {
    assert_eq!("", "I shall not be invoked!");
    None
  }
}

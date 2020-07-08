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
use std::borrow::BorrowMut;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ops::Fn;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, RwLock, RwLockWriteGuard, TryLockError};

pub struct Segment<K, V> {
  inner: RwLock<Inner<K, V>>,
}

struct Inner<K, V> {
  data: HashMap<K, CacheEntry<V>>,
  evictor: ClockEvictor<K>,
}

impl<K, V> Inner<K, V>
where
  K: std::cmp::Eq + std::hash::Hash + Copy,
  V: Clone,
{
  fn update<F>(&mut self, key: K, updating_fn: F) -> Option<V>
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

  fn get_or_populate<F>(&mut self, key: K, populating_fn: F) -> Option<V>
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
  inner: RwLock<InnerSoftLock<V>>,
}

impl<V: Clone> SoftLock<V> {
  fn is_ready(&self) -> bool {
    let guard = self.inner.read().unwrap();
    guard.is_ready()
  }

  fn lock(&self) -> RwLockWriteGuard<InnerSoftLock<V>> {
    self.inner.write().unwrap()
  }

  fn populate<F>(&self, mut guard: RwLockWriteGuard<InnerSoftLock<V>>, value: Option<V>) {
    guard.populate(value);
  }
}

struct InnerSoftLock<V> {
  ready: bool,
  value: Option<V>,
}

impl<V: Clone> InnerSoftLock<V> {
  fn is_ready(&self) -> bool {
    self.ready
  }

  fn value(&self) -> Option<V> {
    self.value.clone()
  }

  fn populate(&mut self, value: Option<V>) {
    if !self.ready {
      self.value = value;
      self.ready = true;
    }
  }
}

impl<K, V> Segment<K, V>
where
  K: std::cmp::Eq + std::hash::Hash + Copy,
  V: Clone,
{
  pub fn new(capacity: usize) -> Segment<K, V> {
    Segment {
      inner: RwLock::new(Inner {
        data: HashMap::new(),
        evictor: ClockEvictor::new(capacity),
      }),
    }
  }

  pub fn get(&self, key: &K) -> Option<V> {
    let guard = self.inner.read().unwrap();
    if let Some(cache_entry) = guard.data.get(key) {
      match cache_entry {
        CacheEntry::Value(entry) => {
          guard.evictor.touch(entry.index);
          return Some(entry.value.clone());
        }
        CacheEntry::Lock(_) => panic!("Fix me!"), // todo Handle lock
      }
    }
    None
  }

  pub fn update<F>(&self, key: K, updating_fn: F) -> Option<V>
  where
    F: Fn(&K, Option<V>) -> Option<V>,
  {
    let mut guard = self.inner.write().unwrap();
    guard.update(key, updating_fn)
  }

  pub fn get_or_populate<F>(&self, key: K, populating_fn: F) -> Option<V>
  where
    F: Fn(&K) -> Option<V>,
  {
    let mut guard = self.inner.write().unwrap();
    guard.get_or_populate(key, populating_fn)
  }

  #[cfg(test)]
  pub fn len(&self) -> usize {
    self.inner.read().unwrap().data.len()
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
    let segment: Segment<i32, String> = test_segment();
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
    let segment: Segment<i32, String> = test_segment();
    let our_key = 42;
    {
      let value = segment.get_or_populate(our_key, miss);
      assert_eq!(value, None);
      assert_eq!(segment.len(), 0);
    }
  }

  #[test]
  fn get_or_populate_evicts() {
    let segment: Segment<i32, String> = test_segment();
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
    let segment: Segment<i32, String> = test_segment();
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
    let segment: Segment<i32, String> = test_segment();
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
    let segment: Segment<i32, String> = test_segment();
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
    let segment: Segment<i32, String> = test_segment();
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

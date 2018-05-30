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

use std::collections::HashMap;
use std::collections::hash_map;
use std::collections::hash_map::Entry;
use std::ops::Fn;
use std::sync::{Arc, RwLock};

pub struct Cachers<K, V> {
  data: RwLock<HashMap<K, Arc<V>>>,
}

impl<K, V> Cachers<K, V>
  where K: std::cmp::Eq + std::hash::Hash,
{
  pub fn new() -> Cachers<K, V>
  {
    Cachers {
      data: RwLock::new(HashMap::new()),
    }
  }

  pub fn get<F>(&self, key: K, populating_fn: F) -> Option<Arc<V>>
    where F: Fn(&K) -> Option<V>,
  {
    match self.data.write().unwrap().entry(key) {
      Entry::Occupied(entry) => {
        Some(entry.get().clone())
      }
      Entry::Vacant(entry) => {
        insert_if_value(populating_fn(entry.key()), entry)
      }
    }
  }

  pub fn update<F>(&self, key: K, updating_fn: F) -> Option<Arc<V>>
    where F: Fn(&K, Option<Arc<V>>) -> Option<V>,
  {
    match self.data.write().unwrap().entry(key) {
      Entry::Occupied(mut entry) => {
        match updating_fn(entry.key(), Some(entry.get().clone())) {
          Some(value) => {
            entry.insert(Arc::new(value));
            Some(entry.get().clone())
          },
          None => {
            entry.remove();
            None
          }
        }
      }
      Entry::Vacant(entry) => {
        insert_if_value(updating_fn(entry.key(), None), entry)
      }
    }
  }

  fn len(&self) -> usize {
    self.data.read().unwrap().len()
  }
}

fn insert_if_value<K, V>(value: Option<V>, entry: hash_map::VacantEntry<K, Arc<V>>) -> Option<Arc<V>>
{
  match value {
    Some(value) => {
      let arc = Arc::new(value);
      entry.insert(arc.clone());
      Some(arc)
    }
    None => None
  }
}

#[cfg(test)]
mod tests {
  use super::Cachers;
  use std::sync::Arc;

  #[test]
  fn hit_populates() {
    let cache: Cachers<i32, String> = Cachers::new();
    let our_key = 42;
    {
      let value = cache.get(our_key, populate);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }

    {
      let value = cache.get(our_key, do_not_invoke);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }
  }

  #[test]
  fn miss_populates_not() {
    let cache: Cachers<i32, String> = Cachers::new();
    let our_key = 42;
    {
      let value = cache.get(our_key, miss);
      assert_eq!(value, None);
      assert_eq!(cache.len(), 0);
    }

    {
      let value = cache.get(our_key, populate);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }
  }

  #[test]
  fn update_populates() {
    let cache: Cachers<i32, String> = Cachers::new();
    let our_key = 42;

    {
      let value = cache.update(our_key, upsert);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }

    {
      let value = cache.get(our_key, do_not_invoke);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }
  }

  #[test]
  fn update_updates() {
    let cache: Cachers<i32, String> = Cachers::new();
    let our_key = 42;

    {
      let value = cache.get(our_key, populate);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }

    {
      let value = cache.update(our_key, update);
      assert_eq!(*value.unwrap(), "42 updated!");
      assert_eq!(cache.len(), 1);
    }

    {
      let value = cache.get(our_key, do_not_invoke);
      assert_eq!(*value.unwrap(), "42 updated!");
      assert_eq!(cache.len(), 1);
    }
  }

  #[test]
  fn update_removes() {
    let cache: Cachers<i32, String> = Cachers::new();
    let our_key = 42;

    {
      let value = cache.get(our_key, populate);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }

    {
      let value = cache.update(our_key, updel);
      assert_eq!(value, None);
      assert_eq!(cache.len(), 0);
    }

    {
      let value = cache.get(our_key, miss);
      assert_eq!(value, None);
      assert_eq!(cache.len(), 0);
    }
  }

  fn miss(_key: &i32) -> Option<String> {
    None
  }

  fn populate(key: &i32) -> Option<String> {
    Some(key.to_string())
  }

  fn upsert(key: &i32, value: Option<Arc<String>>) -> Option<String> {
    assert_eq!(value, None);
    populate(key)
  }

  fn update(_key: &i32, value: Option<Arc<String>>) -> Option<String> {
    let previous = &*value.unwrap();
    Some(previous.clone() + " updated!")
  }

  fn updel(_key: &i32, value: Option<Arc<String>>) -> Option<String> {
    assert!(value.is_some());
    None
  }

  fn do_not_invoke(_key: &i32) -> Option<String> {
    assert_eq!("", "I shall not be invoked!");
    None
  }
}

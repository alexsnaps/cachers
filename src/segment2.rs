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
use crate::softlock::Lock;
use std;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ops::Fn;

use std::sync::RwLock;

use futures::future::Future;

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
  pub fn get(&self, key: &K) -> Option<V> {
    match self.data.get(key) {
      Some(cache_entry) => match cache_entry {
        CacheEntry::Available(v) => {
          self.evictor.touch(v.index);
          return Some(v.value.clone());
        },
        CacheEntry::Locked(_) => todo!("fixme"),
      },
      None => None,
    }
  }

  pub async fn get_or_populate<Fut, F>(&mut self, key: K, populating_fn: F) -> Fut::Output
  where
    F: Fn(K) -> Fut,
    Fut: Future<Output = Option<V>>,
  {
    let (option, key_evicted) = match self.data.entry(key) {
      Entry::Occupied(entry) => match entry.get() {
        CacheEntry::Available(v) => {
          self.evictor.touch(v.index);
          (Some(v.value.clone()), None)
        },
        CacheEntry::Locked(_) => todo!("fixme"),
      }
      Entry::Vacant(entry) => {
        let (option, to_remove) = match populating_fn(*entry.key()).await {
          Some(value) => {
            let (index, to_remove) = self.evictor.add(entry.key().clone());
            let cache_entry = entry.insert(CacheEntry::Available(CacheValue { value, index }));
            match cache_entry {
              CacheEntry::Available(v) => (Some(v.value.clone()), to_remove),
              CacheEntry::Locked(_) => todo!("fixme!"),
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

  pub async fn update<Fut, F>(&mut self, key: K, updating_fn: F) -> Fut::Output
  where
    F: Fn(K, Option<V>) -> Fut,
    Fut: Future<Output = Option<V>>,
  {
    let (option, key_evicted) = match self.data.entry(key) {
      Entry::Occupied(mut entry) => {
        let cache_value = match entry.get_mut() {
          CacheEntry::Available(entry) => entry,
          CacheEntry::Locked(_) => todo!("fix me!"),
        };

        match updating_fn(key, Some(cache_value.value.clone())).await {
          Some(value) => {
            cache_value.value = value;
            self.evictor.touch(cache_value.index);
            (Some(cache_value.value.clone()), None)
          }
          None => {
            entry.remove();
            (None, None)
          }
        }
      },
      Entry::Vacant(entry) => {
        let (option, key_evicted) = match updating_fn(*entry.key(), None).await {
          Some(value) => {
            let (index, to_remove) = self.evictor.add(*entry.key());
            let cache_entry = entry.insert(CacheEntry::Available(CacheValue { value, index }));
            match cache_entry {
              CacheEntry::Available(v) => (Some(v.value.clone()), to_remove),
              CacheEntry::Locked(_) => todo!("fixme"),
            }
          }
          None => (None, None),
        };
        (option, key_evicted)
      }
    };

    if key_evicted.is_some() {
      self.data.remove(&key_evicted.unwrap());
    }

    match option {
      Some(cache_entry) => Some(cache_entry.clone()),
      None => None,
    }
  }
}

enum CacheEntry<V> {
  Available(CacheValue<V>),
  Locked(Lock<V>),
}

struct CacheValue<V> {
  value: V,
  index: usize,
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
    self.inner.read().unwrap().get(key)
  }

  pub async fn get_or_populate<Fut, F>(&self, key: K, populating_fn: F) -> Fut::Output
  where
    F: Fn(K) -> Fut,
    Fut: Future<Output = Option<V>>,
  {
    // let value = self.inner.read().unwrap().get(key);
    // match value {
    //   Some(v) => return v,
    //   None => {
    //     let inner = self.inner.write().unwrap();
    //     // instanciate softlock
    //     // install softlock (inner.put_if_absent())
    //     match inner.data.put_if_absent() {
    //       // si ya rien, create + insert softlock
    //       // si ya softlock
    //     }
    //   },
    // }
    // {
    //   let inner = self.inner.write().unwrap();

    // }

    self.inner.write().unwrap().get_or_populate(key, populating_fn).await
  }

  pub async fn update<Fut, F>(&self, key: K, updating_fn: F) -> Fut::Output
  where
    F: Fn(K, Option<V>) -> Fut,
    Fut: Future<Output = Option<V>>,
  {
    self.inner.write().unwrap().update(key, updating_fn).await
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

  #[tokio::test]
  async fn hit_populates() {
    let segment: Segment<i32, String> = test_segment();
    let our_key = 42;
    {
      let value = segment.get_or_populate(our_key, populate).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.get_or_populate(our_key, do_not_invoke).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }
  }

  #[tokio::test]
  async fn miss_populates_not() {
    let segment: Segment<i32, String> = test_segment();
    let our_key = 42;
    {
      let value = segment.get_or_populate(our_key, miss).await;
      assert_eq!(value, None);
      assert_eq!(segment.len(), 0);
    }
  }

  #[tokio::test]
  async fn get_or_populate_evicts() {
    let segment: Segment<i32, String> = test_segment();
    let our_key = 42;
    {
      let value = segment.get_or_populate(our_key, populate).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
      segment.get_or_populate(2, populate).await;
      segment.get_or_populate(3, populate).await;
      assert_eq!(segment.len(), 3);
      segment.get_or_populate(4, populate).await;
      assert_eq!(segment.len(), 3);
    }
  }

  #[tokio::test]
  async fn update_populates() {
    let segment: Segment<i32, String> = test_segment();
    let our_key = 42;

    {
      let value = segment.update(our_key, upsert).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.get_or_populate(our_key, do_not_invoke).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }
  }

  #[tokio::test]
  async fn update_updates() {
    let segment: Segment<i32, String> = test_segment();
    let our_key = 42;

    {
      let value = segment.get_or_populate(our_key, populate).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.update(our_key, update).await;
      assert_eq!(value.unwrap(), "42 updated!");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.get_or_populate(our_key, do_not_invoke).await;
      assert_eq!(value.unwrap(), "42 updated!");
      assert_eq!(segment.len(), 1);
    }
  }

  #[tokio::test]
  async fn update_evicts() {
    let segment: Segment<i32, String> = test_segment();
    let our_key = 42;
    {
      let value = segment.update(our_key, upsert).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
      segment.update(2, upsert).await;
      segment.update(3, upsert).await;
      assert_eq!(segment.len(), 3);
      segment.update(4, upsert).await;
      assert_eq!(segment.len(), 3);
    }
  }

  #[tokio::test]
  async fn update_removes() {
    let segment: Segment<i32, String> = test_segment();
    let our_key = 42;

    {
      let value = segment.get_or_populate(our_key, populate).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.update(our_key, updel).await;
      assert_eq!(value, None);
      assert_eq!(segment.len(), 0);
    }

    {
      let value = segment.get_or_populate(our_key, miss).await;
      assert_eq!(value, None);
      assert_eq!(segment.len(), 0);
    }
  }

  async fn miss(_key: i32) -> Option<String> {
    None
  }

  async fn populate(key: i32) -> Option<String> {
    Some(key.to_string())
  }

  async fn upsert(key: i32, value: Option<String>) -> Option<String> {
    assert_eq!(value, None);
    populate(key).await
  }

  async fn update(_key: i32, value: Option<String>) -> Option<String> {
    let previous = &value.unwrap();
    Some(previous.clone() + " updated!")
  }

  async fn updel(_key: i32, value: Option<String>) -> Option<String> {
    assert!(value.is_some());
    None
  }

  async fn do_not_invoke(_key: i32) -> Option<String> {
    assert_eq!("", "I shall not be invoked!");
    None
  }
}

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

mod eviction;
mod segment;

use std::ops::Fn;
use std::sync::{Arc, RwLock};

use segment::Segment;

pub struct CacheThrough<K, V> {
  data: RwLock<Segment<K, V>>,
}

impl<K, V> CacheThrough<K, V>
where
  K: std::cmp::Eq + std::hash::Hash + Clone,
{
  pub fn new(capacity: usize) -> CacheThrough<K, V> {
    CacheThrough {
      data: RwLock::new(Segment::new(capacity)),
    }
  }

  pub fn get<F>(&self, key: K, populating_fn: F) -> Option<Arc<V>>
  where
    F: Fn(&K) -> Option<V>,
  {
    if let Some(value) = self.data.read().unwrap().get(&key) {
      return Some(value);
    }

    self.data.write().unwrap().get_or_populate(key, populating_fn)
  }

  pub fn update<F>(&self, key: K, updating_fn: F) -> Option<Arc<V>>
  where
    F: Fn(&K, Option<Arc<V>>) -> Option<V>,
  {
    self.data.write().unwrap().update(key, updating_fn)
  }

  pub fn remove(&self, key: K) {
    self.data.write().unwrap().update(key, |_, _| None);
  }

  fn len(&self) -> usize {
    self.data.read().unwrap().len()
  }
}

#[cfg(test)]
mod tests {
  use super::CacheThrough;
  use std::sync::Arc;

  fn test_cache() -> CacheThrough<i32, String> {
    CacheThrough::new(3)
  }

  #[test]
  fn hit_populates() {
    let cache: CacheThrough<i32, String> = test_cache();
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
    let cache: CacheThrough<i32, String> = test_cache();
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
      cache.get(2, populate);
      cache.get(3, populate);
      // cache.get(4, populate); // todo: don't panic, evict!!!
    }
  }

  #[test]
  fn update_populates() {
    let cache: CacheThrough<i32, String> = test_cache();
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
    let cache: CacheThrough<i32, String> = test_cache();
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
    let cache: CacheThrough<i32, String> = test_cache();
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

  #[test]
  fn remove_removes() {
    let cache: CacheThrough<i32, String> = test_cache();
    let our_key = 42;

    {
      let value = cache.get(our_key, populate);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }

    {
      cache.remove(our_key);
      assert_eq!(cache.len(), 0);
    }
  }

  #[test]
  fn evicts() {
    let cache: CacheThrough<i32, String> = test_cache();

    {
      assert_eq!(*cache.get(1, populate).unwrap(), "1"); // eviction candidate
      assert_eq!(cache.len(), 1);
      assert_eq!(*cache.get(2, populate).unwrap(), "2");
      assert_eq!(cache.len(), 2);
      assert_eq!(*cache.get(3, populate).unwrap(), "3");
      assert_eq!(cache.len(), 3);

      // Clock state & hand:
      // _
      // 111
    }

    {
      assert_eq!(*cache.get(4, populate).unwrap(), "4"); // evicts 1
      assert_eq!(cache.len(), 3);
      //  _
      // 100

      assert_eq!(*cache.get(2, do_not_invoke).unwrap(), "2");
      assert_eq!(cache.len(), 3);
      //  _
      // 110

      assert_eq!(*cache.get(3, do_not_invoke).unwrap(), "3");
      assert_eq!(cache.len(), 3);
      //  _
      // 111
    }

    {
      assert_eq!(*cache.get(5, populate).unwrap(), "5"); // evicts 3
      assert_eq!(cache.len(), 3);
      //   _
      // 010

      assert_eq!(*cache.get(2, do_not_invoke).unwrap(), "2"); // 011
      assert_eq!(cache.len(), 3);
      assert_eq!(*cache.get(4, do_not_invoke).unwrap(), "4"); // 111
      assert_eq!(cache.len(), 3);
    }

    {
      assert_eq!(*cache.get(6, populate).unwrap(), "6"); // evicts 4
      assert_eq!(cache.len(), 3);
      assert_eq!(*cache.get(5, do_not_invoke).unwrap(), "5");
      assert_eq!(cache.len(), 3);
      assert_eq!(*cache.get(2, do_not_invoke).unwrap(), "2");
      assert_eq!(cache.len(), 3);
    }
  }

  #[test]
  fn shared_across_threads() {
    use std::sync::{Arc, Barrier};
    use std::thread;

    let cache: Arc<CacheThrough<i32, String>> = Arc::new(test_cache());
    let other_cache = cache.clone();
    let our_key = 42;

    let barrier = Arc::new(Barrier::new(2));
    let other_barrier = barrier.clone();

    let t = thread::spawn(move || {
      other_barrier.wait();
      let value = other_cache.get(our_key, do_not_invoke);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(other_cache.len(), 1);
    });

    let value = cache.get(our_key, populate);
    assert_eq!(*value.unwrap(), "42");
    assert_eq!(cache.len(), 1);
    barrier.wait();

    t.join().unwrap();
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

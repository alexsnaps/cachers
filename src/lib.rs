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

#![cfg_attr(feature = "unstable", feature(test))]

//! # Cachers [WIP!]
//!
//! `cachers` is a library for in-memory caching in Rust.
//!
//! ## What's in it?
//!
//! It currently only consists of a single `CacheThough` type, that internally uses a `ClockEvictor`
//! for freeing memory when capacity is reached.
//!
//! Eventually this will hopefully become a full set of out-of-the-box ready-to-use caching tools
//!
//! ## Word of warning
//!
//! This is all very much _work in progress_. Fundamentally, it's just me having fun with Rust...
//!

mod eviction;
mod segment;

use std::ops::Fn;
use std::sync::{Arc, RwLock};

use crate::segment::Segment;

/// A thread-safe cache that will populate entries on misses using the provided
/// function, aka a cache-through cache. Uses interior mutability, which means you can simply
/// share a non-mutable reference to both read & insert/update entries to the cache.
///
/// The `CacheThrough` cache uses clock eviction to free elements when it reaches capacity.
///
///
/// In the example below, we create a `CacheThrough` shared across the main thread and another
/// thread we span ourselves. The main thread will populate the `42` entry and the other thread
/// will read it back:
///
/// ```
/// use std::sync::{Arc, Barrier};
/// use std::thread;
///
/// use cachers::CacheThrough;
///
/// let cache: Arc<CacheThrough<i32, String>> = Arc::new(CacheThrough::new(100));
/// let other_cache = cache.clone();
/// let our_key = 42;
///
/// let barrier = Arc::new(Barrier::new(2));
/// let other_barrier = barrier.clone();
///
/// let t = thread::spawn(move || {
///   other_barrier.wait(); // wait for main thread to populate
///   let value = other_cache.get(our_key, |_| unimplemented!() ); // entry should be there!
///   assert_eq!(*value.unwrap(), "42");
/// });
///
/// let value = cache.get(our_key, |key| Some(key.to_string()) ); // miss, so populating
/// assert_eq!(*value.unwrap(), "42");
/// barrier.wait(); // let the other thread proceed
///
/// t.join().unwrap();
/// ```
pub struct CacheThrough<K, V> {
  data: RwLock<Segment<K, V>>,
}

impl<K, V> CacheThrough<K, V>
where
  K: std::cmp::Eq + std::hash::Hash + Copy,
{
  /// Creates a new `CacheThrough` instance of the given `capacity`
  ///
  /// ```
  /// use cachers::CacheThrough;
  ///
  /// let cache = CacheThrough::<usize, String>::new(100);
  /// ```
  pub fn new(capacity: usize) -> CacheThrough<K, V> {
    CacheThrough {
      data: RwLock::new(Segment::new(capacity)),
    }
  }

  /// Retrieves a shared reference to the `V` for the given `key`.
  /// The `populating_fn` will be invoked to populate the cache, should there be no mapping
  /// for the `key` already present. The `populating_fn` receives the `key` as an argument.
  ///
  /// It is guaranteed that `populating_fn` will only be invoked once. That is if multiple thread
  /// race to populate the cache for a given `key`, only one thread will invoke the `populating_fn`.
  /// Once `populating_fn` has returned `Some<V>`, the other threads waiting for the entry to
  /// be populated will get the `Arc<V>` returned.
  ///
  /// In the case where `populating_fn` yield no results (i.e. returns `Option::None`), no
  /// guarantees are made about how many times the `populating_fn` may be called.
  ///
  /// If you want to cache misses, consider wrapping your `V` into an `Option`.
  pub fn get<F>(&self, key: K, populating_fn: F) -> Option<Arc<V>>
  where
    F: Fn(&K) -> Option<V>,
  {
    if let Some(value) = self.data.read().unwrap().get(&key) {
      return Some(value);
    }
    let option = self.data.write();
    if option.is_ok() {
      let mut guard = option.unwrap();
      return guard.get_or_populate(key, populating_fn);
    }
    None
  }

  /// Updates an entry in the cache, or populates it if absent.
  ///
  /// The update can be an actual update or a remove, should the `updating_fn` return a `None`.
  /// The `updating_fn` receives the `key`, as well as an `Option<Arc<V>>` which holds the previous
  /// value for the `key`, which would be `None` if the function is about to populate the cache.
  ///
  /// It is guaranteed that the mapping will not be altered by another thread while the
  /// `populating_fn` executes.
  pub fn update<F>(&self, key: K, updating_fn: F) -> Option<Arc<V>>
  where
    F: Fn(&K, Option<Arc<V>>) -> Option<V>,
  {
    self.data.write().unwrap().update(key, updating_fn)
  }

  /// Removes the entry for `key` from the cache.
  /// This is the equivalent of `cache.update(key, |_, _| None)`. Consider this a convenience method.
  pub fn remove(&self, key: K) {
    self.data.write().unwrap().update(key, |_, _| None);
  }

  #[cfg(test)]
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
      cache.get(4, populate);
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

#[cfg(all(feature = "unstable", test))]
mod bench {
  #![feature(test)]
  extern crate test;
  use std::sync::{Arc, Barrier};
  use std::thread;
  use test::Bencher;

  use crate::CacheThrough;

  #[bench]
  fn get_100_times_no_eviction_two_threads(b: &mut Bencher) {
    let cache_size: i32 = 1000;
    let cache: Arc<CacheThrough<i32, String>> = Arc::new(CacheThrough::new(cache_size as usize));
    let our_key = 42;

    let barrier = Arc::new(Barrier::new(2));

    let other_cache = cache.clone();
    let other_barrier = barrier.clone();
    let t = thread::spawn(move || {
      for warmup in 0..our_key {
        other_cache
          .get(warmup, |key| Some(key.to_string()))
          .expect("We had a miss?!");
      }
      let value = other_cache.get(our_key, |key| Some(key.to_string())); // miss, so populating
      for iteration in 0..10000 {
        {
          other_cache.get(our_key, |_| unimplemented!()).expect("We had a miss?!");
          if iteration % 4 == 0 {
            other_cache
              .update(iteration, |key, _| Some(key.to_string()))
              .expect("We had a miss?!");
          } else {
            other_cache
              .get(iteration as i32, |key| Some(key.to_string()))
              .expect("We had a miss?!");
          }
          if iteration == cache_size / 100 {
            barrier.wait(); // let the other thread proceed
          }
        }
      }
    });
    other_barrier.wait(); // wait for the other thread to populate
    b.iter(|| {
      for _ in 0..100 {
        cache.get(our_key, |_| unimplemented!()).expect("We had a miss?!"); // entry should be there!
      }
    });
    t.join().unwrap();
  }
}

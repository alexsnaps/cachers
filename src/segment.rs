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

use crate::eviction::ClockEvictionStrategy;
use crate::eviction::EvictionStrategy;
use std;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

pub struct Segment<K, V> {
  data: HashMap<K, CacheEntry<V>>,
  eviction_strategy: ClockEvictionStrategy<K>,
}

struct CacheEntry<V> {
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
      data: HashMap::with_capacity(capacity),
      eviction_strategy: ClockEvictionStrategy::new(capacity),
    }
  }

  pub fn get(&self, key: &K) -> Option<V> {
    if let Some(cache_entry) = self.data.get(key) {
      self.eviction_strategy.touch(cache_entry.index);
      return Some(cache_entry.value.clone());
    }
    None
  }

  pub fn write(&mut self, key: K, value: Option<V>) -> Option<V> {
    let (option, key_evicted) = match self.data.entry(key) {
      Entry::Occupied(mut entry) => match value {
        Some(value) => {
          let cache_entry = entry.get_mut();
          cache_entry.value = value;
          self.eviction_strategy.touch(cache_entry.index);
          (Some(cache_entry.value.clone()), None)
        }
        None => {
          entry.remove();
          (None, None)
        }
      },
      Entry::Vacant(entry) => {
        let (option, key_evicted) = match value {
          Some(value) => {
            let (index, to_remove) = self.eviction_strategy.add(*entry.key());
            let cache_entry = entry.insert(CacheEntry { value, index });
            (Some(cache_entry.value.clone()), to_remove)
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
  fn write_populates() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;

    let value = segment.write(our_key, Some(our_key.to_string()));
    assert_eq!(value.unwrap(), "42");
    assert_eq!(segment.len(), 1);
  }

  #[test]
  fn write_evicts() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;
    {
      let value = segment.write(our_key, Some(our_key.to_string()));
      assert_eq!(value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
      segment.write(2, Some(2.to_string()));
      segment.write(3, Some(3.to_string()));
      assert_eq!(segment.len(), 3);
      segment.write(4, Some(4.to_string()));
      assert_eq!(segment.len(), 3);
    }
  }

  #[test]
  fn write_removes() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;

    let value = segment.write(our_key, None);
    assert_eq!(value, None);
    assert_eq!(segment.len(), 0);
  }
}

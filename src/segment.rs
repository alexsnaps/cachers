use eviction::ClockEvictor;
use std;
use std::collections::hash_map;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ops::Fn;
use std::sync::Arc;

pub struct Segment<K, V> {
  capacity: usize,
  data: HashMap<K, CacheEntry<V>>,
  evictor: ClockEvictor<K>,
}

struct CacheEntry<V> {
  value: Arc<V>,
  index: usize,
}

impl<K, V> Segment<K, V>
where
  K: std::cmp::Eq + std::hash::Hash,
{
  pub fn new(capacity: usize) -> Segment<K, V> {
    Segment {
      capacity,
      data: HashMap::new(),
      evictor: ClockEvictor::new(capacity),
    }
  }

  pub fn get(&self, key: &K) -> Option<Arc<V>> {
    if let Some(entry) = self.data.get(key) {
      // todo touch entry
      // self.evictor.touch()
      return Some(entry.value.clone());
    }
    None
  }

  pub fn get_or_populate<F>(&mut self, key: K, populating_fn: F) -> Option<Arc<V>>
  where
    F: Fn(&K) -> Option<V>,
  {
    let (entry_was_present, option) = match self.data.entry(key) {
      Entry::Occupied(entry) => (true, Some(entry.get().value.clone())),
      Entry::Vacant(entry) => (false, insert_if_value(populating_fn(entry.key()), entry)),
    };

    if !entry_was_present && option.is_some() && self.len() > self.capacity {
      self.evict();
    } else if entry_was_present && option.is_some() {
      // todo touch entry
      // self.evictor.touch()
    }

    option
  }

  pub fn update<F>(&mut self, key: K, updating_fn: F) -> Option<Arc<V>>
  where
    F: Fn(&K, Option<Arc<V>>) -> Option<V>,
  {
    let (entry_was_present, option) = match self.data.entry(key) {
      Entry::Occupied(mut entry) => match updating_fn(entry.key(), Some(entry.get().value.clone())) {
        Some(value) => {
          let cache_entry = entry.get_mut();
          cache_entry.value = Arc::new(value);
          (true, Some(cache_entry.value.clone()))
        }
        None => {
          entry.remove();
          (false, None)
        }
      },
      Entry::Vacant(entry) => (false, insert_if_value(updating_fn(entry.key(), None), entry)),
    };

    if !entry_was_present && option.is_some() && self.len() > self.capacity {
      self.evict();
    } else if entry_was_present && option.is_some() {
      // todo touch entry
      // self.evictor.touch()
    }

    match option {
      Some(cache_entry) => Some(cache_entry.clone()),
      None => None,
    }
  }

  pub fn len(&self) -> usize {
    self.data.len()
  }

  fn evict(&mut self) {
    unimplemented!()
  }
}

fn insert_if_value<K, V>(value: Option<V>, entry: hash_map::VacantEntry<K, CacheEntry<V>>) -> Option<Arc<V>> {
  match value {
    Some(value) => {
      let cache_entry = entry.insert(CacheEntry {
        value: Arc::new(value),
        index: 0,
      });
      Some(cache_entry.value.clone())
    }
    None => None,
  }
}

#[cfg(test)]
mod tests {
  use super::Segment;
  use std::sync::Arc;

  fn test_segment() -> Segment<i32, String> {
    Segment::new(3)
  }

  #[test]
  fn hit_populates() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;
    {
      let value = segment.get_or_populate(our_key, populate);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.get_or_populate(our_key, do_not_invoke);
      assert_eq!(*value.unwrap(), "42");
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

    {
      let value = segment.get_or_populate(our_key, populate);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
      segment.get_or_populate(2, populate);
      segment.get_or_populate(3, populate);
      //      segment.get(4, populate); // todo: don't panic, evict!!!
    }
  }

  #[test]
  fn update_populates() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;

    {
      let value = segment.update(our_key, upsert);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.get_or_populate(our_key, do_not_invoke);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }
  }

  #[test]
  fn update_updates() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;

    {
      let value = segment.get_or_populate(our_key, populate);
      assert_eq!(*value.unwrap(), "42");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.update(our_key, update);
      assert_eq!(*value.unwrap(), "42 updated!");
      assert_eq!(segment.len(), 1);
    }

    {
      let value = segment.get_or_populate(our_key, do_not_invoke);
      assert_eq!(*value.unwrap(), "42 updated!");
      assert_eq!(segment.len(), 1);
    }
  }

  #[test]
  fn update_removes() {
    let mut segment: Segment<i32, String> = test_segment();
    let our_key = 42;

    {
      let value = segment.get_or_populate(our_key, populate);
      assert_eq!(*value.unwrap(), "42");
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

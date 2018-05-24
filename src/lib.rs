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

  fn populate(key: &i32) -> Option<String> {
    Some(key.to_string())
  }

  fn do_not_invoke(key: &i32) -> Option<String> {
    assert_eq!("", "I shall not be invoked!");
    Some(key.to_string())
  }

  #[test]
  fn it_works() {
    let cache: Cachers<i32, String> = Cachers::new();
    let our_key = 42;
    {
      let value = cache.get(our_key, populate);
      assert_eq!(*value.unwrap(), "42");
    }

    {
      let value = cache.get(our_key, do_not_invoke);
      assert_eq!(*value.unwrap(), "42");
    }
  }
}

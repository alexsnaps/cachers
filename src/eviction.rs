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
use std::sync::RwLock;

pub trait EvictionStrategy<K> {
  fn add(&mut self, key: K) -> (usize, Option<K>);
  fn touch(&self, index: usize);
}

pub struct ClockEvictionStrategy<K> {
  capacity: usize,
  current_pos: usize,
  clock: RwLock<Vec<bool>>,
  mapping: HashMap<usize, K>,
}

impl<K> ClockEvictionStrategy<K> {
  pub fn new(capacity: usize) -> ClockEvictionStrategy<K> {
    ClockEvictionStrategy {
      capacity,
      current_pos: 0,
      clock: RwLock::new(vec![false; capacity]),
      mapping: HashMap::with_capacity(capacity),
    }
  }

  fn victim(&mut self) -> (usize, Option<K>) {
    let flip_and_match = |touched: &mut bool| {
      let victim = !*touched;
      *touched = false;
      victim
    };
    let mut clock = self.clock.write().unwrap();

    let offset = match clock[self.current_pos..].iter_mut().position(flip_and_match) {
      Some(index) => index,
      None => match clock[..self.current_pos].iter_mut().position(flip_and_match) {
        Some(index) => index,
        None => self.current_pos,
      },
    };
    let mut index = self.current_pos + offset;
    if index >= self.capacity {
      index %= self.capacity;
    }
    self.current_pos = index + 1;
    (index, self.mapping.remove(&index))
  }
}

impl<K> EvictionStrategy<K> for ClockEvictionStrategy<K> {
  fn add(&mut self, key: K) -> (usize, Option<K>) {
    let (index, victim) = if self.mapping.len() < self.capacity {
      (self.mapping.len(), None)
    } else {
      self.victim()
    };

    self.mapping.insert(index, key);
    self.touch(index);
    (index, victim)
  }

  fn touch(&self, index: usize) {
    let mut clock = self.clock.write().unwrap();
    clock[index] = true;
  }
}

mod tests {
  #[allow(unused_imports)]
  use super::{ClockEvictionStrategy, EvictionStrategy};

  #[test]
  fn test_it_works() {
    let mut strategy = ClockEvictionStrategy::new(4);
    assert_eq!(strategy.add("1"), (0, None));
    assert_eq!(strategy.add("2"), (1, None));
    assert_eq!(strategy.add("3"), (2, None));
    assert_eq!(strategy.add("4"), (3, None));
    assert_eq!(strategy.add("5"), (0, Some("1")));
    assert_eq!(strategy.add("6"), (1, Some("2")));
    assert_eq!(strategy.add("7"), (2, Some("3")));
    assert_eq!(strategy.add("8"), (3, Some("4")));
    assert_eq!(strategy.add("9"), (0, Some("5")));

    strategy.touch(1);

    assert_eq!(strategy.add("10"), (2, Some("7")));

    strategy.touch(3);

    assert_eq!(strategy.add("11"), (0, Some("9")));
    assert_eq!(strategy.add("12"), (1, Some("6")));
  }

  #[test]
  fn test_hammered_key_never_evicted() {
    let mut strategy = ClockEvictionStrategy::new(4);
    assert_eq!(strategy.add(1), (0, None));
    strategy.touch(1); // todo, pathological case where the clock goes full circle!
    assert_eq!(strategy.add(2), (1, None));
    strategy.touch(1);
    assert_eq!(strategy.add(3), (2, None));
    strategy.touch(1);
    assert_eq!(strategy.add(4), (3, None));
    for x in 5..10_000 {
      strategy.touch(1);
      assert_ne!(strategy.add(x), (1, Some(2)));
    }
  }
}

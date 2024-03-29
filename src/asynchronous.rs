use futures::future::Future;
use std::ops::Fn;
use std::sync::RwLock;

use crate::segment2::Segment;

pub struct CacheThrough<K, V> {
  data: RwLock<Segment<K, V>>,
}

impl<K, V> CacheThrough<K, V>
where
  K: std::cmp::Eq + std::hash::Hash + Copy,
  V: Clone,
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
  pub async fn get<Fut, F>(&self, key: K, populating_fn: F) -> Fut::Output
  where
    F: Fn(K) -> Fut,
    Fut: Future<Output = Option<V>>,
  {
    if let Some(value) = self.data.read().unwrap().get(&key) {
      return Some(value);
    }

    let option = self.data.write();
    if option.is_ok() {
      let mut guard = option.unwrap();
      return guard.get_or_populate(key, populating_fn).await;
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
  pub async fn update<Fut, F>(&self, key: K, updating_fn: F) -> Option<V>
  where
    F: Fn(K, Option<V>) -> Fut,
    Fut: Future<Output = Option<V>>,
  {
    self.data.write().unwrap().update(key, updating_fn).await
  }

  /// Removes the entry for `key` from the cache.
  /// This is the equivalent of `cache.update(key, |_, _| None)`. Consider this a convenience method.
  pub async fn remove(&self, key: K) {
    self.data.write().unwrap().update(key, |_, _| async { None }).await;
  }

  #[cfg(test)]
  fn len(&self) -> usize {
    self.data.read().unwrap().len()
  }
}

#[cfg(test)]
mod tests {
  use super::CacheThrough;

  fn test_cache() -> CacheThrough<i32, String> {
    CacheThrough::new(3)
  }

  #[tokio::test]
  async fn hit_populates() {
    let cache: CacheThrough<i32, String> = test_cache();
    let our_key = 42;
    {
      let value = cache.get(our_key, populate).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }

    {
      let value = cache.get(our_key, do_not_invoke).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }
  }

  #[tokio::test]
  async fn miss_populates_not() {
    let cache: CacheThrough<i32, String> = test_cache();
    let our_key = 42;
    {
      let value = cache.get(our_key, miss).await;
      assert_eq!(value, None);
      assert_eq!(cache.len(), 0);
    }

    {
      let value = cache.get(our_key, populate).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
      cache.get(2, populate).await;
      cache.get(3, populate).await;
      cache.get(4, populate).await;
    }
  }

  #[tokio::test]
  async fn update_populates() {
    let cache: CacheThrough<i32, String> = test_cache();
    let our_key = 42;

    {
      let value = cache.update(our_key, upsert).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }

    {
      let value = cache.get(our_key, do_not_invoke).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }
  }

  #[tokio::test]
  async fn update_updates() {
    let cache: CacheThrough<i32, String> = test_cache();
    let our_key = 42;

    {
      let value = cache.get(our_key, populate).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }

    {
      let value = cache.update(our_key, update).await;
      assert_eq!(value.unwrap(), "42 updated!");
      assert_eq!(cache.len(), 1);
    }

    {
      let value = cache.get(our_key, do_not_invoke).await;
      assert_eq!(value.unwrap(), "42 updated!");
      assert_eq!(cache.len(), 1);
    }
  }

  #[tokio::test]
  async fn update_removes() {
    let cache: CacheThrough<i32, String> = test_cache();
    let our_key = 42;

    {
      let value = cache.get(our_key, populate).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }

    {
      let value = cache.update(our_key, updel).await;
      assert_eq!(value, None);
      assert_eq!(cache.len(), 0);
    }

    {
      let value = cache.get(our_key, miss).await;
      assert_eq!(value, None);
      assert_eq!(cache.len(), 0);
    }
  }

  #[tokio::test]
  async fn remove_removes() {
    let cache: CacheThrough<i32, String> = test_cache();
    let our_key = 42;

    {
      let value = cache.get(our_key, populate).await;
      assert_eq!(value.unwrap(), "42");
      assert_eq!(cache.len(), 1);
    }

    {
      cache.remove(our_key).await;
      assert_eq!(cache.len(), 0);
    }
  }

  #[tokio::test]
  async fn evicts() {
    let cache: CacheThrough<i32, String> = test_cache();

    {
      assert_eq!(cache.get(1, populate).await.unwrap(), "1"); // eviction candidate
      assert_eq!(cache.len(), 1);
      assert_eq!(cache.get(2, populate).await.unwrap(), "2");
      assert_eq!(cache.len(), 2);
      assert_eq!(cache.get(3, populate).await.unwrap(), "3");
      assert_eq!(cache.len(), 3);

      // Clock state & hand:
      // _
      // 111
    }

    {
      assert_eq!(cache.get(4, populate).await.unwrap(), "4"); // evicts 1
      assert_eq!(cache.len(), 3);
      //  _
      // 100

      assert_eq!(cache.get(2, do_not_invoke).await.unwrap(), "2");
      assert_eq!(cache.len(), 3);
      //  _
      // 110

      assert_eq!(cache.get(3, do_not_invoke).await.unwrap(), "3");
      assert_eq!(cache.len(), 3);
      //  _
      // 111
    }

    {
      assert_eq!(cache.get(5, populate).await.unwrap(), "5"); // evicts 3
      assert_eq!(cache.len(), 3);
      //   _
      // 010

      assert_eq!(cache.get(2, do_not_invoke).await.unwrap(), "2"); // 011
      assert_eq!(cache.len(), 3);
      assert_eq!(cache.get(4, do_not_invoke).await.unwrap(), "4"); // 111
      assert_eq!(cache.len(), 3);
    }

    {
      assert_eq!(cache.get(6, populate).await.unwrap(), "6"); // evicts 4
      assert_eq!(cache.len(), 3);
      assert_eq!(cache.get(5, do_not_invoke).await.unwrap(), "5");
      assert_eq!(cache.len(), 3);
      assert_eq!(cache.get(2, do_not_invoke).await.unwrap(), "2");
      assert_eq!(cache.len(), 3);
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
    panic!("I shall not be invoked!");
  }
}

#[macro_use]
extern crate lazy_static;
extern crate cachers;

use cachers::CacheThrough;
use std::sync::Arc;
use std::thread;

lazy_static! {
  static ref CACHE: Arc<CacheThrough<i32, String>> = Arc::new(CacheThrough::new(2));
}

pub fn main() {
  let our_key = 42;

  let thread_name = "main";

  let value = CACHE.get(our_key, |key| Some(key.to_string())); // miss, so populating
  println!("Value for '{}' on {} is {:?}", our_key, thread_name, *value.unwrap());

  let t = thread::spawn(move || {
    let value = CACHE.get(our_key, |_| unimplemented!()); // entry should be there!
    println!(
      "Value for '{}' on {:?} was present: {:?}",
      our_key,
      thread::current().id(),
      *value.unwrap()
    );
    for offset in 1..3 {
      let new_key = our_key + offset;
      let value = CACHE.get(new_key, |key| Some(key.to_string()));
      println!(
        "Added key '{}' on {:?}: {:?}",
        new_key,
        thread::current().id(),
        *value.unwrap()
      );
    }
  });

  t.join().unwrap(); // … and let it finish

  for offset in 1..3 {
    let new_key = our_key + offset;
    let value = CACHE.get(new_key, |_| unimplemented!()); // entry should be there!
    println!(
      "Value for '{}' on {} was present: {:?}",
      new_key,
      thread_name,
      *value.unwrap()
    );
  }
  let value = CACHE.get(our_key, |_| Some("gone!".to_string())); // Gone!
  println!(
    "Value for '{}' on {} is now {:?}",
    our_key,
    thread_name,
    *value.unwrap()
  );
}

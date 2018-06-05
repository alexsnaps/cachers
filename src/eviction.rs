pub trait Evictor<K> {
  fn add(&self, key: K);
  fn touch(&self, key: &K);
  fn victim(&self) -> K;
}

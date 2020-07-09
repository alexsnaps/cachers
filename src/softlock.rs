use futures::future::Future;
use futures::task::{Context, Poll};
use std::pin::Pin;

struct Softlock<F> {
  f: F,
}

impl<F> Softlock<F> {
  pub fn new(f: F) -> Self {
    Self { f }
  }
}

impl<F> Future for Softlock<F>
  where F: Future,
{
  type Output = F::Output;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    // https://doc.rust-lang.org/nightly/std/pin/index.html#pinning-is-structural-for-field
    let f = unsafe { self.map_unchecked_mut(|s| &mut s.f) };
    f.poll(cx)
  }
}

pub struct Lock<V>(V);

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn softlock() {
    let softlock = Softlock::new(async { Some(String::from("foo")) });

    assert_eq!(String::from("foo"), softlock.await.unwrap());
  }
}

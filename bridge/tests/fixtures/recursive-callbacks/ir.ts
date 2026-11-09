ir 1;

type Next<T> = fn(Next<T>, T) -> T;

class Driver {
  run(step: Next<f64>, x: f64): f64;
}

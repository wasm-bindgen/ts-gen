// Multi-type-param generic dictionary — exercises the all-required
// path (no builder emitted) plus propagation of two type parameters
// through every binding site.

interface ResultBox<T, E> {
  value: T;
  error: E;
}

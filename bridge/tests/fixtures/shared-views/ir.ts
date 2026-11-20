ir 1;

class Probe {
  fill(@js(unshared) v: *mut u8[]);
  see(@js(unshared) v: *u8[]): u32;
  peek(v: *u16[]): u32;
}

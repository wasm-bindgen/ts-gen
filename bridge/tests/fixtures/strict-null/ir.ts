ir 1;

class Probe {
  @js(null) get missing(): str?;
  get loose(): str?;
  @js(null) get count(): u32?;
  get boxed(): @js(null) Str?;
}

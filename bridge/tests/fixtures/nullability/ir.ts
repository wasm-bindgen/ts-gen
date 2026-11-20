ir 1;

class Probe {
  flags(v: @js(null) bool?, w: bool?);
  pair(v: (@js(null) f64?, f64?));
  maybe(v: str??): str??;
  boxed(v: Str??): Str??;
  @js(null) get missing(): str?;
}

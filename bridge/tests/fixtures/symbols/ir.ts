ir 1;

class Symbol {}
interface Shape {}
enum Key {
  Str(str);
  Sym(Symbol);
  Obj(Shape);
}

class Probe {
  key(): Key;
  sym(): Symbol;
}

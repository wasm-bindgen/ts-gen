interface Win {
  a(): void;
}
interface OnlyA {
  x: number;
}
declare var proc: Proc;
interface XV {
  inst(): void;
}
declare var VX: { new (): VX; prototype: VX; stat(): void };
interface XC {
  inst(): void;
}
interface CXConstructor {
  new (): CX;
  readonly prototype: CX;
  stat(): void;
}
declare var CX: CXConstructor;
interface XF {
  a: number;
}
declare function FX(): FX;
interface XS {
  inst(): void;
}
interface XSConstructor {
  new (): XS;
  readonly prototype: XS;
}
declare var XS: XSConstructor;
interface SXConstructor {
  added(): SX;
}

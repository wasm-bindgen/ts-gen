declare module "lib" {
  import { Pkg } from "dep-pkg";
  import { Rel } from "./rel";
  export function fromPackage(p: Pkg): void;
  export function fromRelative(r: Rel): void;
}

import { Rel } from "./rel";

declare module "lib" {
  import { Pkg } from "dep-pkg";
  export function augmented(r: Rel): void;
  export function augmentedPkg(p: Pkg): void;
}

declare module "no-cross-file-names-dep" {
  interface Hidden {
    h: number;
  }
  export {};
}

declare module "no-cross-file-names-dep" {
  export function useHidden(h: Hidden): void;
}

//! @ts-gen --lib-name alias-reexport-cycle --export alias-reexport-cycle

// A bundler renames a declaration and re-exports it under its original name
// inside a namespace. In the namespace `Position` is the re-export, so the
// alias's target only reaches the class when read from the alias's own
// scope; read from the namespace it would name the alias itself, forever.

declare class Position { lineNumber: number; }
type editor_main_Position = Position;
declare namespace editor_main {
  export { editor_main_Position as Position };
  export function at(p: Position): Position;
}
export { editor_main as m };

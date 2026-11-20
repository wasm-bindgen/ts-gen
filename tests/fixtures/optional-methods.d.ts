// An interface of optional methods alone is a dictionary of optional
// function properties: a caller builds it, setting the functions it
// has. Beside a required method they stay methods to call.

interface Chunk {
  readonly text: string;
}

interface Handlers {
  /** Called per chunk. */
  chunk?(chunk: Chunk): void | Promise<void>;
  end?(): void;
  label?: string;
}

interface Rewriter {
  on(selector: string, handlers: Handlers): Rewriter;
  flush?(): void;
}

declare function rewriter(): Rewriter;

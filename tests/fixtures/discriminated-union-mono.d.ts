//! @ts-gen --experimental-generic-mono

// Per-mono, a branch factory takes a field its branch requires bare, even
// where another branch makes it optional and the merged setter takes
// `Option<…>`: `new_inline(content_id: impl JsStringLike, …)` passes
// `Some(content_id)` on.

type Attachment =
  | {
      disposition: "inline";
      contentId: string;
      filename: string;
    }
  | {
      disposition: "attachment";
      contentId?: undefined;
      filename: string;
    };

declare function send(attachment: Attachment): void;

type ReadResult<R = any> =
  | {
      done: false;
      value: R;
    }
  | {
      done: true;
      value?: undefined;
    };

declare function write<R>(result: ReadResult<R>): void;

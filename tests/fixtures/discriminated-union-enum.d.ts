//! @ts-gen --lib-name discriminated-union-enum --export discriminated-union-enum

// Each branch narrows the discriminant to one literal, while the setter a
// branch factory calls is the merged property's, which takes the enum.
export type Attachment =
  | { disposition: "inline"; contentId: string }
  | { disposition: "attachment"; filename: string };

// Integer literal discriminants lower to a C-style enum the same way.
export type Reply =
  | { code: 200; body: string }
  | { code: 404; reason: string };

// A branch field narrowed to one member of the merged union calls that
// member's setter overload.
export type Payload =
  | { kind: "text"; data: string }
  | { kind: "binary"; data: ArrayBuffer };

export declare function send(attachment: Attachment, reply: Reply, payload: Payload): void;

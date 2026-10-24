// A required field with a single literal type offers no choice: the
// literal is baked into every factory without naming the variant after
// it. Only fields with several options (in one dictionary, or across
// the branches of a discriminated union) contribute `_<literal>` suffixes.

interface Placeholder {
  certPresented: "0";
  certVerified: "NONE";
  certIssuerDN: "";
  verified?: boolean;
}

interface Attachment {
  kind: "attachment";
  disposition: "inline" | "attachment";
  filename: string;
}

// Each branch of a discriminated union pins one literal, but across the
// branches it is the choice, so it still names each branch's factory.
type Part =
  | { disposition: "inline"; contentId: string; filename: string }
  | { disposition: "attachment"; contentId?: undefined; filename: string };

// Above the 64-combination factory cap each field keeps only its first
// option, but a literal-union field still offers a choice, so the one
// remaining factory is still named after its literal (`new_inline`).
interface CappedAttachment {
  disposition: "inline" | "attachment";
  a: string | number;
  b: string | number;
  c: string | number;
  d: string | number;
  e: string | number;
  f: string | number;
}

// The IR names an anonymous literal union's enum for its use, so two uses
// of the same literals in one declaration are two enums: each accessor
// reads its own, and a parameter its own beside the result's.

interface Reasoning {
  generateSummary?: "auto" | "concise";
  summary?: "auto" | "concise";
  /** The same name under the same owner shares the property's enum. */
  retry(summary: "auto" | "concise"): void;
  swap(mode: "fast" | "slow"): "fast" | "slow";
}

declare class Pen {
  static tip: "fine" | "broad";
  static cap: "fine" | "broad";
  ink: "fine" | "broad";
}

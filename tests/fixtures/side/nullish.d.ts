interface Lookup {
  /** @tw-nullish */
  readonly cached: string | null;
  /** @tw-nullish */
  find(key: string | null): string | null | undefined;
}

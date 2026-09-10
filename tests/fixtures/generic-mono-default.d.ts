interface DefaultDetails<T> {
  value: T;
}

declare class DefaultFlags {
  getStringDetails(defaultValue: string): Promise<DefaultDetails<string>>;
  getBooleanDetails(defaultValue: boolean): Promise<DefaultDetails<boolean>>;
  getNumberDetails(defaultValue: number): Promise<DefaultDetails<number>>;
  getStringArray(): Promise<Array<string>>;
}

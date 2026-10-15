//! @ts-gen --export tests/fixtures/static-names.d.ts --export mod-a --export mod-b
declare const $: unknown;
declare const $foo: string;
declare let fooBar: number;
declare var FOO_BAR: number;
declare const déjà: string;
declare var console: string;
declare var console: string;

declare module "mod-a" {
    export const value: string;
}
declare module "mod-b" {
    export const value: string;
}

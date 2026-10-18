// An optional `any` property reads as a bare `JsValue`, which carries
// `undefined` in-band (`cf() -> JsValue`), under per-monomorphization too.
interface ResponseInit {
  status?: number;
  cf?: any;
  // Under per-monomorphization an optional property that can only be
  // absent has no accessors.
  error?: never;
}
declare class Reply {
  constructor(init?: ResponseInit);
  readonly init: ResponseInit;
}

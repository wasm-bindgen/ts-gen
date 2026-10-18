// An optional `any` property reads as a bare `JsValue`, which carries
// `undefined` in-band (`cf() -> JsValue`), under per-monomorphization too.
interface ResponseInit {
  status?: number;
  cf?: any;
}
declare class Reply {
  constructor(init?: ResponseInit);
  readonly init: ResponseInit;
}

const describe = (values) =>
  `${Array.isArray(values) ? "Array" : values.constructor.name}(${Array.from(values, String).join(",")})`;

let kept;
export const keepView = (data) => {
  kept = data;
};
export const keepCopy = (data) => {
  kept = data;
};
export const keepUnsharedView = (data) => {
  kept = data;
};
export const keepSharedView = (data) => {
  kept = data;
};
export const first = () => Number(kept[0]);
export const entries = describe;
export const digest = describe;
export const describeTarget = (target) => describe(target.buffer);

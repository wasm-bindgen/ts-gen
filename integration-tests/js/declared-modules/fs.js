export const constants = { O_RDONLY: 1 };
export function open(path) {
  return { fd: path.length };
}
